use anyhow::{Context, Result};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{FxIndexSet, ResolvedVc, ValueToString, Vc};
use turbo_tasks_fs::{FileContent, FileSystemPath};
use turbo_tasks_hash::{DeterministicHasher, Xxh3Hash64Hasher};
use turbopack_core::{
    asset::{Asset, AssetContent},
    chunk::{Chunk, ChunkingContext, ContentHashing, OutputChunk, OutputChunkRuntimeInfo},
    ident::AssetIdent,
    introspect::{Introspectable, IntrospectableChildren},
    output::{OutputAsset, OutputAssetsReference, OutputAssetsWithReferenced},
    source_map::{GenerateSourceMap, SourceMapAsset},
    version::VersionedContent,
};
use turbopack_ecmascript::chunk::EcmascriptChunk;

use crate::{BrowserChunkingContext, ecmascript::content::EcmascriptBrowserChunkContent};

/// Development Ecmascript chunk.
#[turbo_tasks::value(shared)]
#[derive(ValueToString)]
#[value_to_string("Ecmascript Dev Chunk")]
pub struct EcmascriptBrowserChunk {
    chunking_context: ResolvedVc<BrowserChunkingContext>,
    chunk: ResolvedVc<EcmascriptChunk>,
}

#[turbo_tasks::value_impl]
impl EcmascriptBrowserChunk {
    /// Creates a new [`Vc<EcmascriptDevChunk>`].
    #[turbo_tasks::function]
    pub fn new(
        chunking_context: ResolvedVc<BrowserChunkingContext>,
        chunk: ResolvedVc<EcmascriptChunk>,
    ) -> Vc<Self> {
        EcmascriptBrowserChunk {
            chunking_context,
            chunk,
        }
        .cell()
    }

    #[turbo_tasks::function]
    async fn source_map(self: Vc<Self>) -> Result<Vc<SourceMapAsset>> {
        let this = self.await?;
        Ok(SourceMapAsset::new(
            Vc::upcast(*this.chunking_context),
            this.ident_for_path().await?,
            Vc::upcast(self),
        ))
    }
}

impl EcmascriptBrowserChunk {
    async fn component_chunk_assets(&self) -> Result<Vec<ResolvedVc<Box<dyn OutputAsset>>>> {
        let component_chunks = self.chunk.component_chunks().await?;
        let mut assets = Vec::with_capacity(component_chunks.len());
        for &component in component_chunks.iter() {
            let component_chunk = ResolvedVc::try_downcast_type::<EcmascriptChunk>(component)
                .context("merged chunk component_chunks must be ecmascript chunks")?;
            assets.push(ResolvedVc::upcast(
                EcmascriptBrowserChunk::new(*self.chunking_context, *component_chunk)
                    .to_resolved()
                    .await?,
            ));
        }
        Ok(assets)
    }

    pub(crate) fn ecmascript_chunk(&self) -> ResolvedVc<EcmascriptChunk> {
        self.chunk
    }

    pub(crate) async fn ident_for_path(&self) -> Result<Vc<AssetIdent>> {
        Ok(self
            .chunk
            .ident()
            .owned()
            .await?
            .with_modifier(rcstr!("ecmascript dev chunk"))
            .into_vc())
    }
}

#[turbo_tasks::value_impl]
impl OutputChunk for EcmascriptBrowserChunk {
    #[turbo_tasks::function]
    async fn runtime_info(&self) -> Result<Vc<OutputChunkRuntimeInfo>> {
        let component_assets = self.component_chunk_assets().await?;
        let module_chunks = if component_assets.is_empty() {
            None
        } else {
            Some(ResolvedVc::cell(component_assets))
        };
        Ok(OutputChunkRuntimeInfo {
            included_ids: Some(self.chunk.entry_ids().to_resolved().await?),
            module_chunks,
            ..Default::default()
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptBrowserChunk {
    #[turbo_tasks::function]
    pub(crate) async fn own_content(self: Vc<Self>) -> Result<Vc<EcmascriptBrowserChunkContent>> {
        let this = self.await?;
        Ok(EcmascriptBrowserChunkContent::new(
            *this.chunking_context,
            self,
            this.chunk.chunk_content(),
            self.source_map(),
        ))
    }

    #[turbo_tasks::function]
    pub fn chunk(&self) -> Result<Vc<Box<dyn Chunk>>> {
        Ok(Vc::upcast(*self.chunk))
    }
}

#[turbo_tasks::value_impl]
impl OutputAssetsReference for EcmascriptBrowserChunk {
    #[turbo_tasks::function]
    async fn references(self: Vc<Self>) -> Result<Vc<OutputAssetsWithReferenced>> {
        let this = self.await?;
        let chunk_references = this.chunk.references().await?;
        let include_source_map = *this
            .chunking_context
            .reference_chunk_source_maps(Vc::upcast(self))
            .await?;
        let ref_assets = chunk_references.assets.await?;
        let mut assets =
            Vec::with_capacity(ref_assets.len() + if include_source_map { 1 } else { 0 });

        assets.extend(ref_assets.iter().copied());

        if include_source_map {
            assets.push(ResolvedVc::upcast(self.source_map().to_resolved().await?));
        }

        // Constituent component chunks of a merged chunk are emitted as referenced assets
        // so the runtime can fetch an individual component when it's already cached, without
        // them being eagerly loaded as primary chunks.
        let component_assets = this.component_chunk_assets().await?;
        let referenced_assets = if component_assets.is_empty() {
            chunk_references.referenced_assets
        } else {
            let mut referenced: Vec<_> = chunk_references
                .referenced_assets
                .await?
                .iter()
                .copied()
                .collect();
            referenced.extend(component_assets);
            ResolvedVc::cell(referenced)
        };

        Ok(OutputAssetsWithReferenced {
            assets: ResolvedVc::cell(assets),
            referenced_assets,
            references: chunk_references.references,
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl OutputAsset for EcmascriptBrowserChunk {
    #[turbo_tasks::function]
    async fn path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        let this = self.await?;
        let path_info = this.chunking_context.chunk_path_info().await?;

        let name = match path_info.chunk_content_hashing {
            Some(ContentHashing::Direct { length }) => {
                // Two-level hashing so mutual async-loader references cannot
                // form a turbo-tasks cycle:
                //
                //  Level 1 – hash of estimated chunk content (no cross-chunk
                //  path embeddings).  Computed independently per chunk.
                //
                //  Level 2 – hash of (salt || sorted closure of level-1
                //  hashes of every browser chunk reachable through output-
                //  asset references).  Depends only on level-1 hashes, which
                //  are all independent, so there is no cycle.  Hashing the
                //  full closure means any reachable content change propagates
                //  to this chunk's path — preserving the original cache-
                //  busting semantics.

                // BFS over output-asset references to collect the closure.
                let mut seen: FxIndexSet<ResolvedVc<EcmascriptBrowserChunk>> =
                    FxIndexSet::default();
                let mut queue: Vec<ResolvedVc<EcmascriptBrowserChunk>> =
                    vec![self.to_resolved().await?];
                while let Some(chunk) = queue.pop() {
                    if !seen.insert(chunk) {
                        continue;
                    }
                    let refs = chunk.references().await?;
                    for &asset in refs.assets.await?.iter() {
                        if let Some(bc) =
                            ResolvedVc::try_downcast_type::<EcmascriptBrowserChunk>(asset)
                        {
                            if !seen.contains(&bc) {
                                queue.push(bc);
                            }
                        }
                    }
                }

                // Collect and sort level-1 hashes from the closure.
                let mut l1s: Vec<u64> = Vec::with_capacity(seen.len());
                for chunk in &seen {
                    l1s.push(*chunk.base_hash().await?);
                }
                l1s.sort();

                let own_l1 = self.base_hash().await?;
                let salt = this.chunking_context.hash_salt().await?;
                let mut hasher = Xxh3Hash64Hasher::new();
                hasher.write_value(salt.as_str());
                // Include this chunk's level-1 hash first so siblings
                // within the same SCC get distinct filenames.
                hasher.write_value(*own_l1);
                for l1 in &l1s {
                    hasher.write_value(*l1);
                }
                let l2 = hasher.finish();

                let hash = turbo_tasks_hash::encode_base38(l2);
                let hash = &hash[..length as usize];
                format!("{hash}.js").into()
            }
            None => {
                let ident = this.ident_for_path().await?;
                ident
                    .output_name(path_info.root_path.clone(), None, rcstr!(".js"))
                    .owned()
                    .await?
            }
        };

        Ok(path_info.chunk_root_path.join(&name)?.cell())
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptBrowserChunk {
    /// Level-1 (base) hash: estimated chunk code without cross-chunk path
    /// embeddings.  Safe to compute independently — no dependency on any
    /// other chunk's path.
    #[turbo_tasks::function]
    pub(crate) async fn base_hash(self: Vc<Self>) -> Result<Vc<u64>> {
        let content = self.own_content().estimated_code();
        let rope = content.to_rope_with_magic_comments(|| self.source_map()).await?;
        let mut hasher = Xxh3Hash64Hasher::new();
        DeterministicHasher::write_bytes(&mut hasher, rope.to_bytes().as_ref());
        Ok(Vc::cell(hasher.finish()))
    }
}

#[turbo_tasks::value_impl]
impl Asset for EcmascriptBrowserChunk {
    #[turbo_tasks::function]
    fn content(self: Vc<Self>) -> Vc<AssetContent> {
        self.own_content().content()
    }

    #[turbo_tasks::function]
    fn versioned_content(self: Vc<Self>) -> Vc<Box<dyn VersionedContent>> {
        Vc::upcast(self.own_content())
    }
}

#[turbo_tasks::value_impl]
impl GenerateSourceMap for EcmascriptBrowserChunk {
    #[turbo_tasks::function]
    fn generate_source_map(self: Vc<Self>) -> Vc<FileContent> {
        self.own_content().generate_source_map()
    }

    #[turbo_tasks::function]
    fn by_section(self: Vc<Self>, section: RcStr) -> Vc<FileContent> {
        self.own_content().by_section(section)
    }
}

#[turbo_tasks::value_impl]
impl Introspectable for EcmascriptBrowserChunk {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<RcStr> {
        Vc::cell(rcstr!("dev ecmascript chunk"))
    }

    #[turbo_tasks::function]
    fn title(self: Vc<Self>) -> Vc<RcStr> {
        self.path().to_string()
    }

    #[turbo_tasks::function]
    fn details(&self) -> Vc<RcStr> {
        Vc::cell(rcstr!("generates a development ecmascript chunk"))
    }

    #[turbo_tasks::function]
    fn children(&self) -> Result<Vc<IntrospectableChildren>> {
        let mut children = FxIndexSet::default();
        let chunk = ResolvedVc::upcast::<Box<dyn Introspectable>>(self.chunk);
        children.insert((rcstr!("chunk"), chunk));
        Ok(Vc::cell(children))
    }
}
