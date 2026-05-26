# Unsafe code audit — working findings

This file is the **running notebook** of the unsafe audit. After all files in
`unsafe-todo.md` are processed, this file will be rewritten into a polished
report.

Per-finding entries use the following structure:

```
### <id> <Short title> — <severity>
- Location: `path/to/file.rs:LINE` (and others if relevant)
- Category: <Send/Sync | transmute | FFI | from_raw | …>
- Claim:    <what the unsafe block is meant to guarantee>
- Issue:    <what is wrong, missing, or under-documented>
- Severity: Critical | High | Medium | Low | Note
- Notes:    <optional caller analysis / suggested fix>
```

Severity rubric:

- **Critical** — known unsoundness reproducible from safe API surface, UB likely in practice.
- **High** — likely unsoundness; needs only a plausible caller to trigger.
- **Medium** — fragile invariant relying on undocumented caller behavior, or unsoundness only with `unsafe` callers; or platform-specific risk.
- **Low** — minor: missing `SAFETY:` comment, suboptimal API, defense-in-depth.
- **Note** — sound but worth recording (e.g. macro-generated `unsafe impl`).

---

## Findings

### F-001 `marker_trait::impl_auto_marker_trait!` blanket impls — Note (sound)

- Location: `turbopack/crates/turbo-tasks/src/marker_trait.rs:12-92`
- Category: `unsafe impl` for `NonLocalValue`/`OperationValue`
- Claim: blanket impls of two `unsafe trait`s (`NonLocalValue`, `OperationValue`) for
  every common container/wrapper type (`Vec<T>`, `Option<T>`, `Arc<T>`, `Box<T>`,
  `Mutex<T>`, `Pin<T: Deref>`, `Cow<T>`, `Either`, tuples, fn-pointers, `&T`/`&mut T`
  …). Generic impls require `T: $trait`.
- Issue: none — these marker traits are "no `Vc` (and optionally no `ResolvedVc`)
  reachable inside the type". The `T: $trait` bound transitively propagates that
  guarantee. The unconditional `unsafe impl<T: ?Sized> $trait for PhantomData<T> {}`
  (line 75) is fine because `PhantomData` is a ZST that holds no value of `T`. The
  `&T`/`&mut T` impls (lines 89-90) are sound because references hold no `Vc` (the
  pointee may, but that's covered by the `T: $trait` bound — and the lifetime
  system prevents storing them across task boundaries).
- Severity: Note

### F-002 `RcStr` tagged-pointer Send/Sync — Note (sound)

- Location: `turbopack/crates/turbo-rcstr/src/lib.rs:88-98`
- Category: `unsafe impl Send/Sync` for a tagged-pointer ABI struct.
- Claim: `RcStr` is `Send + Sync` because every backing variant is:
  - `STATIC_TAG` → `&'static StaticPrehashedString` (Send+Sync)
  - `DYNAMIC_TAG` → `triomphe::Arc<DynamicPrehashedString>` (Send+Sync because
    `Arc<T: Send+Sync>` is, and `DynamicPrehashedString` is `Box<str> + u64`)
  - `INLINE_TAG` → inline bytes (`Send+Sync`)
- Issue: none, but the unsafe impl is unconditional. A future change that adds a
  non-`Send`/`Sync` payload to `DynamicPrehashedString` would silently break this
  invariant. The `unsafe_data: TaggedValue` field is named to flag this, but a
  `SAFETY:` block enumerating the three tag arms would harden maintenance.
- Severity: Low (defense-in-depth)

### F-003 `RcStr::clone` Arc-count manipulation — Note (sound)

- Location: `turbopack/crates/turbo-rcstr/src/lib.rs:322-339`
- Category: `Arc::from_raw` + `forget` to dup a count without dropping.
- Claim: `restore_arc` is `Arc::from_raw`, which adopts one strong count. Cloning
  bumps to +1, then both `arc.clone()` and `arc` are `forget`'d to keep both counts
  alive: original `RcStr` keeps its 1, new `RcStr` gets its own 1.
- Issue: subtle but correct. Worth a SAFETY comment explaining the count math
  (the existing comment focuses on which tag, not on why double-`forget` is correct).
- Severity: Low (clarity)

### F-004 `RcStr::new_atom_from_prehashed` alignment asymmetry — Low

- Location: `turbopack/crates/turbo-rcstr/src/dynamic.rs:74-88`
- Category: pointer tagging.
- Claim: `Arc::into_raw(...)` returns an aligned pointer; the low 2 bits are then
  OR'd with `DYNAMIC_TAG`. The assertion is a runtime `debug_assert!`.
- Issue: `new_static_atom` (same file, line 91-107) uses a `const`-block assertion
  on `align_of::<StaticPrehashedString>() >= 4`. The dynamic path lacks a matching
  compile-time alignment assertion on `DynamicPrehashedString` (currently
  alignment 8 because it contains a `u64` and `Box<str>`, but nothing structural
  enforces it). A future repr-change that drops `u64` for two `u16`s would silently
  lower alignment and break the tag bits — only caught in `debug_assert` builds.
  Add `const { assert!(align_of::<DynamicPrehashedString>() >= 4); }`.
- Severity: Low (latent / hardening)

### F-005 `tagged_value::TaggedValue::new_tag` transmute — Note (sound)

- Location: `turbopack/crates/turbo-rcstr/src/tagged_value.rs:82-88`
- Category: `transmute` between `u64`/`usize`/`u128` and `NonZeroU64`/`NonNull`/…
- Claim: input is `NonZeroU8::get() as RawTaggedValue`, so non-zero.
- Issue: For the `usize`→`NonNull<()>` case (default 64-bit target), the transmute
  is layout-compatible. Provenance is integer-only here; the pointer is never
  dereferenced when the tag is `INLINE_TAG`. For `STATIC_TAG`/`DYNAMIC_TAG`, the
  pointer was originally obtained via `Arc::into_raw`/`&'static`, so dereferencing
  via `deref_static`/`deref_dynamic`/`restore_arc` carries provenance through the
  `value.cast()` / `usize → *mut` chain. Sound under Rust's current loose provenance
  rules.
- Severity: Note

### F-006 `CrateMapWrapper`/`RegularMapWrapper` Send+Sync — Medium

- Location: `turbopack/crates/turbopack-core/src/source_map/mod.rs:649-675`
- Category: `unsafe impl Send/Sync` for a non-Send/Sync upstream type.
- Claim (from doc): "Must not use per line access to the SourceMap, as it is not
  thread safe." `DecodedMap` from `sourcemap` is not `Send`/`Sync` because it
  contains internal state (interner / cache) using `Cell`/`RefCell`/raw pointers.
- Issue: The `unsafe impl` is unconditional, but the SAFETY comment relies on
  callers avoiding "per line access". There is no actual API gate — `Deref<Target =
DecodedMap>` lets any caller invoke any `&self` method on `DecodedMap`,
  including ones that internally mutate via `Cell`. The wrapper does prune
  to `RegularMap` via `as_regular_source_map()`, but `Deref` is still public, and
  `lookup_token`/related methods that race on caches would be UB under concurrent
  reads from two threads. Note however that `DecodedMap` cached state may actually
  be `Mutex`-protected in newer `sourcemap` versions (depends on the pinned
  version) — needs verification at the actual pinned version.
- Severity: Medium (latent — depends on upstream)
- Fix suggestion: hide `Deref<Target = DecodedMap>` and only expose verified-
  thread-safe accessors; or wrap in a `Mutex` and remove the unsafe impl.

### F-007 `CodeGenResultComments` Send+Sync — Note (redundant, sound)

- Location: `turbopack/crates/turbopack-ecmascript/src/lib.rs:2713-2714`
- Category: `unsafe impl Send/Sync`.
- Claim: not stated.
- Issue: `CodeGenResultComments` consists of `Either<ImmutableComments,
Arc<ImmutableComments>>`, `SwcComments` (= `Arc<DashMap<...>>`), `Vec<Module-
Position>`, `Arc<Mutex<Vec<…>>>`, and a recursive `Vec<Self>`. All of those are
  already `Send + Sync` — the explicit impls are redundant. The danger: a future
  refactor that adds a non-Send/!Sync field (e.g. an `Rc` or a raw pointer) will
  silently keep `Send + Sync` and produce a quiet data race. Replace with auto-
  derive (remove the impls) or guard with a `static_assert::assert_impl_all!`.
- Severity: Note (clarity / future-proofing)

### F-008 `dash_map_multi::RefMut: Sync` — Note (sound; well-documented)

- Location: `turbopack/crates/turbo-tasks-backend/src/utils/dash_map_multi.rs:13-45,113-224`
- Category: `unsafe impl Sync` for a guard type wrapping raw `Bucket`.
- Claim: `RefMut: !Send` (compiler-enforced via `PhantomData<*const ()>` in `Shared`
  and via parking_lot's `RwLockWriteGuard` in `Simple`); `RefMut: Sync` because
  `pair()` only yields `&V` (gated by `V: Sync`) and `pair_mut`/`value_mut` take
  `&mut self`. `get_multiple_mut` asserts that the two returned `RefMut`s have
  distinct bucket pointers (line 167) before returning them, so even when two
  `RefMut`s share an `Arc<RwLockWriteGuard>`, they cannot alias their `V`.
- Issue: the safety story holds. The aliasing guard is the runtime assert at line
  166-169, which would _panic_ on accidental equal-key calls, preventing the only
  way to construct overlapping `Shared` refs. The shard-locking dance in lines
  185-198 needs the always-write-larger-shard-first / try / retry pattern, which is
  present and prevents lock cycles.
- Severity: Note
- Suggestion: keep the explicit `unsafe impl Sync` even though `V: Sync` is the
  only delicate bound; consider replacing the runtime `assert!` with a
  `debug_assert!` only after fuzzing the safety net — but the assert is cheap and
  catches a memory-safety bug at the point of construction. The current form is
  preferable.

### F-009 `OnceConcurrentlyMap` `'a -> 'static` transmute — Medium (sound but subtle)

- Location: `turbopack/crates/turbo-tasks/src/once_map.rs:52-70`
- Category: lifetime laundering via `std::mem::transmute`.
- Claim: `TemporarilyInserted` inserts a `&'a K` masqueraded as `&'static K` into a
  `FxDashMap<&'static K, …>`. `Drop` removes the entry, so the dangling reference
  is never observed after `'a`.
- Issue: soundness requires:
  1. No `mem::forget(temp)` of `TemporarilyInserted`. Local-only, so true.
  2. The closure `func()` must not panic between `entry()` and `Drop` in a way
     that breaks the unwind path. `Drop` runs on unwind, so this is safe.
  3. No other thread reads the `&'static K` _outside_ of the dashmap's shard
     lock. DashMap's `entry()` API only exposes the key via the shard guard;
     `.contains/.get` etc. compare via `Eq` while holding the shard lock, so
     the key reference is never escaped. Sound.
  4. The thread doing Drop must complete before `'a` ends — that's a single-stack-
     frame invariant. Sound.
- Severity: Note (sound, but document the subtle reasoning)
- Suggestion: ensure `TemporarilyInserted` is `!Send` (it is, via `&'a K`'s
  variance). Add a doc comment to `entry()` describing that the &'static masquerade
  is only safe because Drop cleans up.

### F-010 `Scope::new` unsafe constructor — Note (sound)

- Location: `turbopack/crates/turbo-tasks/src/scope.rs:173-189,213-225`
- Category: lifetime erasure of a captured FnOnce closure for tokio spawn.
- Claim: `Scope` lifetimes `'scope: 'env`; the constructor is `unsafe` so the only
  caller (`scope_and_block`) is responsible for guaranteeing `Drop` runs. `Drop`
  waits for all spawned tasks via the `ScopeInner` join/condvar dance and then
  rethrows the first panic. The closure transmute from `'scope` to `'static` is
  safe because Drop synchronizes before `'scope` ends.
- Issue: standard scoped-thread pattern, mirrors `std::thread::scope`. The
  invariance over `'env` (`PhantomData<&'env mut &'env ()>`) is correctly applied
  to prevent lifetime shrinking. `Scope::new` is `unsafe fn` so a caller who
  `mem::forget`s the Scope would cause UB — `scope_and_block` is the only call
  site and does not forget.
- Severity: Note (well-designed, but if `Scope::new` were ever made public, it
  should keep its `unsafe` marker and rationale comment expanded).

### F-011 `IntoChunks` / `Chunk` shared `Arc<Vec<SyncUnsafeCell<…>>>` — Note (sound)

- Location: `turbopack/crates/turbo-tasks/src/util.rs:283-393`
- Category: shared mutable aliasing via `SyncUnsafeCell<ManuallyDrop<T>>`.
- Claim: `into_chunks(Vec<T>, chunk_size)` reinterprets the buffer as
  `SyncUnsafeCell<ManuallyDrop<T>>` (layout-compatible repr-transparent wrappers)
  and gives out non-overlapping `Chunk { data: Arc<…>, index, end }` views. Each
  `Chunk::next()` calls `ManuallyDrop::take` on `data[index]`, then `self.index +=
1`. Disjoint ranges enforced by `IntoChunks::next()` advancing past each chunk.
- Issue: soundness rests on:
  1. Chunks issued via `IntoChunks::next()` always have disjoint `[index, end)`
     ranges (true by construction).
  2. `IntoChunks::drop()` only drains items beyond `self.index`, i.e. items not
     handed to any Chunk (true).
  3. `Chunk::drop()` only takes from `index..end`, both of which mutate only
     within the chunk's range (true).
  4. `T: Send`/`Sync` not asserted explicitly — relies on auto-trait. Since
     `SyncUnsafeCell<T>` is `Sync` unconditionally and `Send if T: Send`, the
     resulting `Chunk<T>: Send iff T: Send`. Correct.
- Severity: Note (sound; relies on subtle range-disjointness invariant)
- Suggestion: add a SAFETY block to `into_chunks` summarizing the disjointness
  invariant.

### F-012 `id.rs` `Deref` via `transmute_copy(&&NonZero<T>) -> &T` — Note (sound)

- Location: `turbopack/crates/turbo-tasks/src/id.rs:72-79`
- Category: `transmute_copy` between layout-compatible reference types.
- Claim: `NonZero<T>` has the same layout as `T` (Rust guarantee), so
  `&NonZero<T>` and `&T` are layout-compatible.
- Issue: the `transmute_copy` actually takes `&&NonZero<T>` (a double reference)
  and produces `&T`. Both are pointer-sized; the input `&&NonZero<T>` carries the
  address of the inner `&NonZero<T>`. Returning a `&T` to that address is sound
  because the local lives for the function call. But `transmute_copy::<&&X, &Y>`
  is unusual; `transmute::<&X, &Y>` would be more conventional and equally sound.
- Severity: Note (style)
- Suggestion: prefer plain `&*(self.id as *const NonZero<$primitive>).cast::<$primitive>()`,
  or simply `&self.id.get()` if the underlying type allows it (it doesn't, because
  `.get()` returns by value, not by reference). The current form is sound but
  surprising.

### F-013 `triomphe_utils::unchecked_sidecast_triomphe_arc` — Note (sound under documented contract)

- Location: `turbopack/crates/turbo-tasks/src/triomphe_utils.rs:21-45,49-52`
- Category: `Arc::into_raw`/`from_raw` between different `T`/`U`.
- Claim: caller guarantees "safe to transmute from T to U". For triomphe's
  `Arc<T>` representation (`#[repr(C)] ArcInner<T> { count, data: T }`), the
  negative offset from `data` to `count` is `max(align_of::<AtomicUsize>(),
align_of::<T>())`. If `align_of::<U>() != align_of::<T>()`, `from_raw` could
  compute the wrong header offset.
- Issue: callers in this repo:
  1. `downcast_triomphe_arc`: `dyn Any` to concrete T after `Any::is::<T>` check.
     Because the original arc was created from a concrete-type-T allocator, the
     alignment matches. Sound.
  2. `coerce_to_any_send_sync` via `Coercion::new(|x| x)`: identity coercion to
     `dyn Any + Send + Sync`. Layout preserved. Sound.
- Severity: Note (sound; safety bound `T:U layout-compat` is enforced via callers).

### F-014 `vc/read.rs` `transmute_copy::<ManuallyDrop<&T>, &Target>` — Note (sound when generated by macro)

- Location: `turbopack/crates/turbo-tasks/src/vc/read.rs:111-145`
- Category: generic transmute for `#[turbo_tasks::value(transparent)]` types.
- Claim: the `#[turbo_tasks::value(transparent)]` macro guarantees that `T` and
  `Target` are both `#[repr(transparent)]` wrappers of the same underlying type.
  `transmute_copy::<ManuallyDrop<X>, Y>` works because `ManuallyDrop<X>` is
  `#[repr(transparent)]` over `X`.
- Issue: soundness depends entirely on the macro's correctness. A hand-written
  `impl VcValueType for Foo { type Read = VcTransparentRead<Foo, Inner>; … }`
  where `Foo` and `Inner` are _not_ layout-compatible would cause UB. The trait
  is `unsafe trait VcValueType`, which transfers the bound to the impl writer.
- Severity: Note (sound; relies on macro/safety contract).

### F-015 `dynamic.rs` `bytes.as_ptr().add(offset)` then `*const [u8; N]` deref — Note (sound under doc'd bound)

- Location: `turbopack/crates/turbo-rcstr/src/dynamic.rs:181-204,225-272`
- Category: unaligned read of `[u8; 8]` / `[u8; 4]` from slice via raw-pointer cast.
- Claim: the `[u8; N]` array has alignment 1 (matches `u8`), so the cast and deref
  are aligned. `from_le_bytes(*array)` produces a `u64`/`u32` from those bytes.
- Issue: sound. The `debug_assert!(offset + 8 <= bytes.len())` is a redundant
  release-mode bug catcher; in release, an out-of-bounds offset would call
  `add` past end-of-allocation which is UB. The hash function's structure
  appears to keep offsets in bounds (`if len >= 8 { read(0); read(len-8) }`).
- Severity: Note

### F-016 `MetaFile`/`MetaEntry` self-referential mmap + `'static`-erased borrow — Medium (sound but fragile)

- Location: `turbopack/crates/turbo-persistence/src/meta_file.rs:111-123,228-257,316-329`
- Category: `mem::transmute` to extend a borrow lifetime to `'static`; self-
  referential struct relying on field drop order; `unsafe impl Send/Sync`.
- Claim (well-documented):
  - `entries: Vec<MetaEntry>` holds `qfilter::FilterRef<'static>` that actually
    borrow from `mmap: Mmap` in the same `MetaFile`.
  - Rust drops fields in declaration order, so `entries` is declared before
    `mmap`, ensuring all FilterRefs drop before the mmap unmaps.
  - `Mmap` from `memmap2` stores a raw pointer to OS-mapped memory; moving the
    `Mmap` doesn't relocate the mapped bytes, so a moved `MetaFile` keeps the
    borrows valid.
  - `unsafe impl Send/Sync for MetaEntry`: `FilterRef<'static>` is a read-only
    view; `qfilter::FilterRef` is `Send + Sync` provided its underlying types
    are. (Verified against published `qfilter` crate.)
- Issue: soundness rests on:
  1. Field order in `MetaFile` (commented as such, but Rust does not enforce this
     with an attribute — reordering is a silent UB regression).
  2. `Mmap` not being modified by another process; not enforced. The wider
     turbo-persistence design serializes writes, so OK.
  3. `qfilter::FilterRef` not containing self-pointers into its backing slice
     (presumed; would need re-audit on qfilter upgrade).
- Severity: Medium (sound under contract; documented; but no machine-checked
  drop-order guard).
- Suggestion: add `#[non_exhaustive]` or a `static_assertions::assert_fields!` to
  pin the field order; consider a wrapper type like
  `selfref::SelfRef<Mmap, FilterRef>` to eliminate the manual `'static` transmute.

### F-017 `MetaFile::open_internal` `Mmap::map(&file)` — Note (sound; documented assumption)

- Location: `turbopack/crates/turbo-persistence/src/meta_file.rs:271`
- Category: `memmap2::Mmap::map` (unsafe because mapped bytes can change if
  another process writes to the file).
- Claim: turbo-persistence is single-writer; meta files are immutable once
  written (atomic rename).
- Severity: Note

### F-018 `static_sorted_file.rs::Mmap::map` and `slice_from_subslice` — Note (sound; same single-writer assumption)

- Location: `turbopack/crates/turbo-persistence/src/static_sorted_file.rs:224,512,651,676,720,786,946`
- Category: `Mmap::map` and `ArcBytes`/`RcBytes` raw-pointer subslicing.
- Claim: similar to `MetaFile`; SST files are immutable once written.
- Severity: Note

### F-019 `arc_bytes.rs` / `rc_bytes.rs` `unsafe impl Send + Sync` + raw pointer — Note (sound)

- Location: `turbopack/crates/turbo-persistence/src/arc_bytes.rs:35-36,57,117,134`;
  `turbopack/crates/turbo-persistence/src/rc_bytes.rs:54,90,107`
- Category: shared bytes wrapper around an Arc/Rc + raw pointer subslice.
- Claim: ArcBytes holds an `Arc<Mmap>` plus a `*const [u8]` into it; Send/Sync
  because the Arc keeps the mmap alive and the underlying mmap is immutable.
- Issue: SAFETY block on `unsafe impl Send/Sync` is the brief "see file comment"
  pattern. Sound but a one-line SAFETY note inline would help.
- Severity: Note

### F-020 `dash_map_raw_entry.rs` `bucket.as_ref/as_mut` under shard guard — Note (sound)

- Location: `turbopack/crates/turbo-tasks-backend/src/utils/dash_map_raw_entry.rs:37,101,123,138`
- Category: hashbrown raw bucket dereferencing under a held shard guard.
- Claim: bucket pointer obtained via `find_or_find_insert_slot`/`insert_in_slot`
  while holding the shard write guard; `&self`/`&mut self` on the wrapper type
  prevents alias.
- Severity: Note

### F-021 `storage.rs` `shard_guard.iter()` and `bucket.as_ref/as_mut` — Note (sound under guard)

- Location: `turbopack/crates/turbo-tasks-backend/src/backend/storage.rs:307,310,430,521,523,559`
- Category: raw bucket iteration under a held lock guard.
- Severity: Note

### F-022 `kv_backing_storage.rs` / `write_batch.rs` `WriteBatch::flush` — Medium (caller-contract)

- Location: `turbopack/crates/turbo-persistence/src/write_batch.rs:260`;
  `turbopack/crates/turbo-tasks-backend/src/database/turbo/mod.rs:239-240`;
  `turbopack/crates/turbo-tasks-backend/src/database/write_batch.rs:62`;
  `turbopack/crates/turbo-tasks-backend/src/database/noop_kv.rs:63`;
  `turbopack/crates/turbo-tasks-backend/src/kv_backing_storage.rs:301,451,561`;
  `turbopack/crates/turbo-persistence/src/tests.rs:158`
- Category: `unsafe fn flush(&self, family: u32)`.
- Claim: `flush` takes `&self` (not `&mut self`) but says caller must guarantee
  the family isn't being written by any other thread. The trait/impl uses
  `unsafe fn` to surface this requirement to callers. Looking at `write_batch.rs:139`
  the implementation calls `unsafe { &mut *cell.get() }` on an `UnsafeCell<State>`.
- Issue: the unsafe contract requires "no concurrent writers to `family`". All
  current callers appear to satisfy this. The contract is documented (see file
  header). Concern: if `flush` is ever invoked from a non-coordinated context,
  it would alias a `&mut State` with concurrent writers' `&mut State`. Today's
  callers are serial.
- Severity: Medium (high-impact if violated; low-likelihood given current usage).
- Suggestion: consider taking `&mut self` and pushing the per-family
  coordination up one level (each family in its own `WriteBatch`).

### F-023 `local_task_tracker.rs` `LocalTaskId::new_unchecked` — Note (sound)

- Location: `turbopack/crates/turbo-tasks/src/local_task_tracker.rs:54`
- Category: `LocalTaskId::new_unchecked(self.tasks.len() as u32)` — relies on
  `self.tasks.len()` always being > 0 at the call site (the slot must exist).
- Severity: Note

### F-024 `value_type.rs` `SyncUnsafeCell<u16>` for `ValueTypeId` — Note (sound; serialized by init)

- Location: `turbopack/crates/turbo-tasks/src/value_type.rs:236,251,338-340`
- Category: `SyncUnsafeCell` writes during `init_registry`, reads via
  `LazyLock::force` happens-before, plus a `transmute_copy(&&self.id)` style.
- Severity: Note

### F-025 `turbopack-trace-server/chunked_vec.rs` `MaybeUninit::assume_init_*` — Note (sound)

- Location: `turbopack/crates/turbopack-trace-server/src/chunked_vec.rs:37,81,107,121,131`
- Category: `MaybeUninit<T>` paired with `len` tracking. Pattern: writes through
  `Cell<u32>` len followed by `assume_init_*` only for `idx < len`.
- Issue: standard idiom; sound assuming len monotonically tracks initialized
  slots in append-only fashion. Looking at the code, the chunked vec writes
  before incrementing len — appears correct.
- Severity: Note

### F-026 `turbo-tasks-malloc::TurboMalloc` GlobalAlloc — Note (sound)

- Location: `turbopack/crates/turbo-tasks-malloc/src/lib.rs:108,153-197`
- Category: custom global allocator wrapper that tracks allocation sizes.
- Claim: wraps `mimalloc::MiMalloc` or `std::alloc::System`. Forwards alloc/dealloc/
  realloc/alloc_zeroed and updates a counter. The `Layout::from_size_align_unchecked`
  reuse for the new layout in `realloc` uses `layout.align()` (already valid).
- Issue: when `mimalloc` is enabled, `mi_usable_size` returns the actual allocation
  size which may be larger than `layout.size()`. The counter therefore tracks
  realized memory, not requested memory. Sound; not a correctness issue.
- Severity: Note

### F-027 `turbo-tasks-malloc::counter.rs::NonNull::new_unchecked(ptr)` — Note (sound)

- Location: `turbopack/crates/turbo-tasks-malloc/src/counter.rs:120-121`
- Category: thread-local allocation counter access via `NonNull::new_unchecked`
  on a pointer that's guaranteed non-null (initialized prior).
- Severity: Note

### F-028 `turbo-bincode::macro_helpers` `transmute<&mut E, &mut TurboBincodeEncoder>` — Medium (suspect)

- Location: `turbopack/crates/turbo-bincode/src/macro_helpers.rs:30,53`
- Category: `transmute<&mut E, &mut TurboBincodeEncoder>` between generic encoder
  types. Same for `&mut D` -> `&mut TurboBincodeDecoder<'a>`.
- Issue: this is a type-erased downcast. If `E` is not actually `TurboBincodeEncoder`,
  this is UB. The function should be `unsafe fn`, or use a runtime type-id check
  (e.g. `unty::type_equal`). Needs deeper inspection.
- Severity: deferred until inspection.

### F-029 `value_trait_macro.rs` etc. macro-emitted `unsafe impl Upcast/Dynamic` — Note (relies on macro)

- Location: `turbopack/crates/turbo-tasks-macros/src/value_trait_macro.rs:293-387`;
  `turbopack/crates/turbo-tasks-macros/src/value_impl_macro.rs:341,343`;
  `turbopack/crates/turbo-tasks-macros/src/value_macro.rs:593`;
  `turbopack/crates/turbo-tasks-macros/src/derive/{operation_value,non_local_value}_macro.rs`
- Category: macro-emitted `unsafe impl` for `VcValueType`/`Upcast`/`UpcastStrict`/
  `Dynamic`/`OperationValue`/`NonLocalValue` traits.
- Claim: the macros emit `unsafe impl` only after the user has used the macro
  (which is documented as the only sound way to implement the trait).
- Issue: relies on macro correctness. The macro asserts compile-time bounds on
  field types (`NonLocalValue` derive asserts every field is `NonLocalValue`),
  but `Upcast`/`Dynamic` are emitted only inside the macro at the trait declaration
  site, so they're scoped to the user-declared trait hierarchy. Sound.
- Severity: Note

### F-030 `turbo-bincode/src/lib.rs:116` SliceReader unsafe block — Note (sound)

- Location: `turbopack/crates/turbo-bincode/src/lib.rs:103-130`
- Category: `copy_nonoverlapping` after a `split_at_checked` bounds check.
- Claim: head and target_buffer are non-overlapping slices (one is a read from
  self.buffer, the other is the caller-provided buffer; aliasing would violate
  Rust's borrowing rules).
- Severity: Note

### F-031 `versioned_content_map.rs` HACK `unsafe impl OperationValue` — Medium (documented hack)

- Location: `crates/next-api/src/versioned_content_map.rs:28,63`
- Category: `unsafe impl OperationValue for MapEntry`/`PathToOutputOperation`
  where the inner contains `ResolvedVc`/`FxHashMap<…ResolvedVc…>`.
- Claim: comment says "This is technically incorrect because path_to_asset
  contains ResolvedVc..." — `OperationValue` is defined as "does not contain any
  instances of `Vc` or `ResolvedVc`; may contain `OperationVc`". This violates
  the trait's invariant.
- Issue: per the runtime assertions in turbo-tasks documentation, this might
  fail with a runtime panic rather than UB ("There are currently runtime
  assertions in place as a fallback to ensure memory safety, but those
  assertions may become debug-only in the future" — see `vc/local.rs:28-29`).
  Once the runtime assertions become debug-only, this `unsafe impl` would
  produce UB when the `ResolvedVc` is stored in a State across task boundaries
  and serialized/deserialized — the inner `ResolvedVc` would not get re-resolved
  on read.
- Severity: Medium (a real future-UB hazard; explicitly flagged by author)
- Suggestion: refactor `path_to_asset` to use `FxHashMap<FileSystemPath,
OperationVc<Box<dyn OutputAsset>>>` or store path/asset in a side table; or
  convince yourself that the path-to-asset map is reconstructed on each task
  invocation and remove the State storage.

### F-032 `dotenv.rs::env::set_var/remove_var` — Note (sound under documented lock)

- Location: `turbopack/crates/turbo-tasks-env/src/dotenv.rs:96,102`
- Category: `unsafe fn std::env::{set,remove}_var` (Rust 1.84+ marked these
  as `unsafe` because they're not thread-safe).
- Claim: caller holds `&MutexGuard<()>` to serialize all env mutations.
- Issue: this serializes against other Turbopack callers of `set_var`. But
  process-global env state is not protected against parallel calls from other
  crates (e.g. via libc setenv from a C library) or from JS via napi. Within
  this crate's domain, sound.
- Severity: Note

### F-033 `LazySortedVec` Send/Sync + UnsafeCell + Once — Note (sound; subtle)

- Location: `turbopack/crates/turbopack-trace-server/src/lazy_sorted_vec.rs:11-58`
- Category: `unsafe impl Send/Sync` for `UnsafeCell<SmallVec<[T;1]>>` plus
  `Once`-gated interior mutation in `deref`.
- Claim: deref accesses `vec.get()` then `once.call_once(|| sort_and_shrink)`.
  Multiple concurrent `&self` derefs are serialized at `call_once`: only one
  thread runs the sort, the rest block until it completes. After that, all
  threads read the now-sorted SmallVec via `&*ptr`. `Once`'s acquire/release
  semantics publish the writes from the sorting thread to readers.
- Issue: subtle. `Once::call_once` _must_ be the only path that mutates inside
  `&self` access. The mutating methods (`push`, `retain_unordered`,
  `iter_mut_unordered`) all take `&mut self`, which excludes concurrent `&self`
  derefs via Rust's borrow checker. Plus `push` resets `self.once = Once::new()`
  so the next deref re-sorts. Sound.
- Severity: Note

### F-034 `trace_writer::WriteGuard::buffer().unwrap_unchecked()` — Note (sound)

- Location: `turbopack/crates/turbopack-trace-utils/src/trace_writer.rs:230-242`
- Category: `Option::unwrap_unchecked` on an Option that's been initialized to
  Some at the start of the struct's life and only ever replaced by Some.
- Severity: Note

### F-035 `span_ref.rs` / `span_graph_ref.rs` / `span_bottom_up_ref.rs` `SpanId::new_unchecked` — Note (sound)

- Location: `turbopack/crates/turbopack-trace-server/src/span_ref.rs:38`;
  `…/span_graph_ref.rs:37`; `…/span_bottom_up_ref.rs:25`
- Category: `SpanId::new_unchecked(self.index << 1 [| 1])` constructing a
  `NonZeroU32`/`NonZeroU64` from a (shifted) span index.
- Claim: requires the shifted value to be non-zero. Index 0 with `<< 1` would
  produce 0, which would be UB.
- Issue: looking at the construction, span 0 likely exists in the store as a
  valid first entry. Then `SpanRef.id() = SpanId::new_unchecked(0 << 1) = 0` —
  that's a NonZero violation. **Suspicious.** I'd need to verify whether index 0
  is reserved.
- Severity: deferred / needs follow-up

### F-036 `reader::turbopack::transmute<&mut Vec<TraceRow<'_>>, &mut Vec<TraceRow<'_>>>` — Note (sound)

- Location: `turbopack/crates/turbopack-trace-server/src/reader/turbopack.rs:498-499`
- Category: identity transmute that the compiler should already accept; the
  comment indicates a lifetime erasure (the input and output are spelled the
  same in source but elaborated to different lifetimes by inference).
- Claim: the Vec is emptied at the end of the function, so no `TraceRow<'_>`
  with the laundered lifetime escapes.
- Severity: Note

### F-037 `turbo-rcstr/src/lib.rs:napi_impl` — Note (sound)

- Location: `turbopack/crates/turbo-rcstr/src/lib.rs:611-628`
- Category: NAPI boundary; wraps `String::from_napi_value` etc.
- Severity: Note

### F-038 `next_api/utils.rs ToNapiValue for TurbopackResult` — Note (sound)

- Location: `crates/next-napi-bindings/src/next_api/utils.rs:386-410`
- Category: NAPI boundary; uses `JsUnknown::from_raw`, `cast::<JsObject>` after
  type-check via `result.get_type()`.
- Issue: the `unsafe { result.cast::<JsObject>() }` is gated by a runtime
  `ValueType::Object` check (line 396), which is correct usage of NAPI's
  `cast` API.
- Severity: Note

### F-039 `next-napi-bindings turbopack.rs from_napi_value for NapiRouteHas` — Note (sound)

- Location: `crates/next-napi-bindings/src/turbopack.rs:162-188`
- Category: NAPI boundary; standard manual `FromNapiValue` implementation.
- Severity: Note

### F-040 `rspack/crates/binding/src/lib.rs:107` plugin registration — Note (sound)

- Location: `rspack/crates/binding/src/lib.rs:107`
- Category: NAPI `FromNapiValue::from_napi_value(env.raw(), object.raw())` —
  standard pattern for the rspack plugin macro.
- Severity: Note

### F-041 `turbopack-ecmascript` various `unsafe impl NonLocalValue` — Note (sound)

- Location: `turbopack/crates/turbopack-ecmascript/src/utils.rs:250` (AstSyntaxContext);
  `…/references/mod.rs:3718` (AstPath);
  `…/module_graph/{traced_di_graph,chunk_group_info,mod}.rs:53,51,92`;
  `…/dev-server/src/update/stream.rs:36`;
  `…/turbopack-core/src/resolve/alias_map.rs:73`;
  `turbo-tasks/src/graph/adjacency_map.rs:18`;
  `turbo-tasks/src/id.rs:153`;
  `turbo-tasks/src/invalidation.rs:58,61`.
- Category: `unsafe impl NonLocalValue` / `OperationValue` for various types
  that wrap third-party data (RoaringBitmap, swc syntax types) and only contain
  primitive/static/`'static`-wrapper data.
- Severity: Note (sound; the trait's invariant "no `Vc` or `ResolvedVc`
  reachable in the type" is upheld in each case via inspection).

### F-042 `next-napi-bindings utils.rs ToNapiValue::to_napi_value` — see F-038

### F-043 `db.rs::Mmap::map`, `bin/sst_inspect.rs::Mmap::map`, `static_sorted_file.rs` mmap — see F-018

### F-044 `string::from_utf8_unchecked` in base38, minify, BytesStr — Note (sound by construction)

- Location: `turbopack/crates/turbo-tasks-hash/src/base38.rs:61`;
  `turbopack/crates/turbopack-ecmascript/src/minify.rs:202`;
  `turbopack/crates/turbo-rcstr/src/lib.rs:219`;
  `turbopack/crates/turbo-tasks/src/macro_helpers.rs:132` (`const_concat`);
  `turbopack/crates/turbopack-ecmascript/src/tree_shake/graph.rs:1737`.
- Category: `String::from_utf8_unchecked` / `str::from_utf8_unchecked` where
  the input is constructed from a known-ASCII alphabet (BASE38, BASE54) or
  comes from a verified-UTF-8 source (SWC emitter output, `BytesStr`).
- Severity: Note

### F-045 `turbopack-trace-server::chunked_vec.rs` line 37 unsafe block — Note (sound)

- Location: `turbopack/crates/turbopack-trace-server/src/chunked_vec.rs:36-39`
- Category: array-of-`MaybeUninit` allocation init.
- Severity: Note (see F-025)

### F-046 `auto_hash_map::map.rs:659,731` `&mut *this` from raw — Note (sound)

- Location: `turbopack/crates/turbo-tasks-auto-hash-map/src/map.rs:659,731`
- Category: raw `*mut AutoMap` dereference inside `VacantEntry::insert` and
  `VacantRawEntry::insert`. The raw pointer was captured at entry creation;
  the `'a` lifetime of the entry forbids any other `&mut AutoMap` access in
  the meantime.
- Issue: standard `MapEntry`-style pattern. Correct.
- Severity: Note

### F-047 `compression.rs` `Arc::new_uninit_slice` + `assume_init` — Note (sound)

- Location: `turbopack/crates/turbo-persistence/src/compression.rs:34-50`
- Category: `Arc<[MaybeUninit<u8>]>` allocated, then `decompress_block` writes
  the full slice (verified by length check), then `assume_init`.
- Issue: the order is `assume_init` _before_ `decompress_block` (line 37,
  before the write at line 40). Strictly speaking, `assume_init` for
  `[MaybeUninit<u8>]` is sound for `u8` (no invalid bit patterns), but the
  general rule is "assume_init only after full initialization". For `u8` only,
  this is fine — and the subsequent `decompress_block(block, dest, …)` writes
  all bytes before the buffer escapes the function. Sound for `u8` but the
  pattern is slightly upside-down. Same for `Rc` path.
- Severity: Note

### F-048 `priority_runner.rs::drop_in_place + write` for in-place future replacement — Note (sound)

- Location: `turbopack/crates/turbo-tasks/src/priority_runner.rs:309-313`
- Category: drop a pinned future in place, write a new one in the same slot.
- Claim: a worker holding a pinned future drops it, then writes a new future at
  the same address (which is still pinned). The new future starts its life
  pinned.
- Issue: subtle but correct, as long as the new future doesn't observe state
  from the previous future. The `*mut E::Future = future_slot` and
  `future_slot.write(new_future)` writes via raw pointer, bypassing the borrow
  checker. The pin contract requires that the memory location remains pinned
  until the future is dropped — here it's dropped before the new future is
  written, so the new future immediately becomes pinned at the same address.
  Sound.
- Severity: Note

### F-049 `read_ref.rs` `transmute_copy<&ReadTarget, &'static ReadTarget>` — Note (sound)

- Location: `turbopack/crates/turbo-tasks/src/read_ref.rs:171`
- Category: lifetime laundering for iterator construction.
- Claim: the `read_ref` field of `ReadRefIter` keeps the underlying Arc alive,
  so the laundered `'static` reference remains valid for the iterator's
  lifetime.
- Issue: standard self-referential iterator pattern. Sound provided iter is
  dropped before read_ref, which is the case via field declaration order.
- Severity: Note

### F-051 `rope.rs::Rope::decode` uninit `Vec::set_len` then read — High (latent UB)

- Location: `turbopack/crates/turbo-tasks-fs/src/rope.rs:430-449`
- Category: `Vec::with_capacity(length)` + `set_len(length)` + `reader.read(&mut bytes)`.
- Claim (in code comment): "`read` writes to (does not read) `bytes` and will
  return an error if exactly length bytes is not written, so no uninitialized
  memory ever escapes this function." This claim is a _one-impl-specific_
  promise.
- Issue: `Decode<Context>` is implemented over arbitrary `D: Decoder<Context =
Context>`. The `Decoder::reader().read(target_buffer)` API takes `&mut [u8]`,
  and the function signature does **not** forbid reading from `target_buffer`
  before writing. A user-provided `Reader` impl that hashes or memcmps the
  buffer prior to filling it would read uninitialized memory — UB. (Reading
  uninitialized memory through `&mut [u8]` is UB regardless of whether the
  result is observed externally; see `https://doc.rust-lang.org/std/mem/union.MaybeUninit.html`.)
- In-tree audit: turbo-bincode's `TurboBincodeReader::read` uses
  `copy_nonoverlapping(head.as_ptr(), target_buffer.as_mut_ptr(), len)` which
  writes (does not read) target_buffer — see F-030. Bincode's default
  `SliceReader` is similar. So no current caller actually exploits this.
- Severity: High (no current UB, but the safety claim is wrong in general).
- Suggestion: change the contract to use `read_into_uninit` (bincode's
  uninit-aware API, if present), or buffer through `[u8; 256]` chunks, or
  zero-initialize via `vec![0u8; length]` (the `clippy::uninit_vec` allow on
  line 431 already acknowledges the problem).

### F-052 `turbo-bincode::macro_helpers::transmute<&mut E, &mut TurboBincodeEncoder>` — Note (sound under `unty::type_equal`)

- Location: `turbopack/crates/turbo-bincode/src/macro_helpers.rs:17-37,42-62`
- Category: gated type-erased downcast of a generic encoder/decoder.
- Claim: `unty::type_equal::<E, TurboBincodeEncoder>()` checks via type id;
  if true, the encoder is the same type, transmute is sound.
- Issue: `unty::type_equal` is documented to compare `TypeId` (or similar)
  excluding lifetimes. Lifetimes do affect references' layouts in some
  positions (e.g. when references contain trait objects with lifetime
  bounds), but for plain `&mut E` where `E` doesn't carry a lifetime
  parameter that affects layout, this is sound.
- The comment in `decode_for_turbo_bincode_decode_impl` claims transmute
  `&'a mut D -> &'a mut TurboBincodeDecoder<'a>`. `TurboBincodeDecoder<'a>` is
  the type alias for an `EncoderImpl<TurboBincodeReader<'a>, …>`. If the
  user's `D` has a different (longer) lifetime, layout is identical but the
  laundered lifetime `'a` may borrow data shorter than the original. Within
  the function body, the reference cannot escape, and any error would be
  caught by `type_equal` returning false → `unreachable!()`.
- Severity: Note (sound; well-documented; relies on `unty`'s correctness).
- Suggestion: marking these `unsafe fn` makes the contract explicit (currently
  they're safe wrappers); not strictly necessary since the `unsafe` block is
  internal.

### F-053 `next_api/utils.rs:392-394` `JsUnknown::from_raw` — Note (sound)

- Location: `crates/next-napi-bindings/src/next_api/utils.rs:392-394`
- Category: NAPI `JsUnknown::from_raw(env, result)` after `T::to_napi_value`.
- Claim: `to_napi_value` returns a valid `napi_value` for `env`; `from_raw`
  adopts it.
- Severity: Note
