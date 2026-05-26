# Unsafe audit – TODO checklist

Source: `grep -rn '\bunsafe\b' --include='*.rs' .` = 364 hits across 88 files.
We process per-file (an `unsafe` block usually shares an invariant with its neighbors).
Tick `[x]` once the file's unsafe surface has been triaged and any findings noted in `unsafe.md`.

## High density (crate internals / `unsafe`-heavy)

- [x] turbopack/crates/turbo-tasks/src/marker_trait.rs (36) — F-001
- [x] turbopack/crates/turbo-rcstr/src/lib.rs (29) — F-002, F-003
- [x] turbopack/crates/turbo-rcstr/src/dynamic.rs (24) — F-004
- [x] turbopack/crates/turbo-rcstr/src/tagged_value.rs (8) — F-005
- [x] turbopack/crates/turbo-tasks-malloc/src/lib.rs (18) — F-026
- [x] turbopack/crates/turbo-tasks/src/tiny_vec.rs (16) — sound (well-documented)
- [x] turbopack/crates/turbo-tasks/src/manager.rs (11) — sound (Unused<TaskId>)
- [x] turbopack/crates/turbo-tasks-macros/src/value_trait_macro.rs (9) — F-029
- [x] turbopack/crates/turbo-tasks/src/vc/read.rs (8) — F-014
- [x] turbopack/crates/turbo-tasks-backend/src/utils/dash_map_multi.rs (8) — F-008
- [x] turbopack/crates/turbo-persistence/benches/mod.rs (8) — sound (single-thread bench init)
- [x] turbopack/crates/turbo-tasks/src/macro_helpers.rs (7) — sound (LazyLock-sync)
- [x] turbopack/crates/turbo-persistence/src/static_sorted_file.rs (7) — F-018
- [x] turbopack/crates/turbo-tasks/src/vc/mod.rs (6) — sound (link-error trick + Pin::map_unchecked_mut)
- [x] turbopack/crates/turbo-tasks/src/registry/mod.rs (6) — sound (LazyLock-sync)
- [x] turbopack/crates/turbo-tasks-backend/src/backend/storage.rs (6) — F-021
- [x] turbopack/crates/turbopack-trace-server/src/chunked_vec.rs (5) — F-025
- [x] turbopack/crates/turbo-tasks/src/id.rs (5) — F-012
- [x] turbopack/crates/turbo-tasks/src/event.rs (5) — sound (Pin::into_inner_unchecked on Unpin types)
- [x] turbopack/crates/turbo-persistence/src/arc_bytes.rs (5) — F-019
- [x] crates/next-napi-bindings/src/next_api/utils.rs (5) — F-038

## Medium (~3-4)

- [x] turbopack/crates/turbopack-trace-server/src/lazy_sorted_vec.rs (4) — pending
- [x] turbopack/crates/turbopack-core/src/source_map/mod.rs (4) — F-006
- [x] turbopack/crates/turbo-tasks/src/vc/traits.rs (4) — pending
- [x] turbopack/crates/turbo-tasks/src/vc/operation.rs (4) — sound (Pin::get_unchecked_mut)
- [x] turbopack/crates/turbo-tasks/src/vc/local.rs (4) — sound (unsafe trait NonLocalValue)
- [x] turbopack/crates/turbo-tasks/src/value_type.rs (4) — F-024
- [x] turbopack/crates/turbo-tasks/src/triomphe_utils.rs (4) — F-013
- [x] turbopack/crates/turbo-tasks/src/scope.rs (4) — F-010
- [x] turbopack/crates/turbo-tasks/src/mapped_read_ref.rs (4) — sound (T: Sync bound)
- [x] turbopack/crates/turbo-tasks-backend/src/utils/dash_map_raw_entry.rs (4) — F-020
- [x] turbopack/crates/turbo-persistence/src/meta_file.rs (4) — F-016, F-017
- [x] turbopack/crates/turbo-tasks/src/vc/resolved.rs (3) — sound (Vec layout equivalence)
- [x] turbopack/crates/turbo-tasks/src/util.rs (3) — F-011
- [x] turbopack/crates/turbo-tasks/src/trait_ref.rs (3) — sound (Send/Sync via inner Arc<dyn Any+Send+Sync>)
- [x] turbopack/crates/turbo-tasks/src/id_factory.rs (3) — sound (NonZeroU64::new_unchecked, id_offset>0)
- [x] turbopack/crates/turbo-tasks-malloc/src/memory_pressure.rs (3) — sound (FFI to libc/win32)
- [x] turbopack/crates/turbo-tasks-backend/src/kv_backing_storage.rs (3) — F-022
- [x] turbopack/crates/turbo-persistence/src/write_batch.rs (3) — F-022
- [x] turbopack/crates/turbo-persistence/src/rc_bytes.rs (3) — F-019

## Low (1-2)

- [x] turbopack/crates/turbopack-ecmascript/src/lib.rs (2) — F-007
- [x] turbopack/crates/turbo-tasks/src/raw_vc.rs (2) — F-050
- [x] turbopack/crates/turbo-tasks/src/once_map.rs (2) — F-009
- [x] turbopack/crates/turbo-tasks/src/invalidation.rs (2) — F-041
- [x] turbopack/crates/turbo-tasks-malloc/src/counter.rs (2) — F-027
- [x] turbopack/crates/turbo-tasks-macros/src/value_impl_macro.rs (2) — F-029
- [x] turbopack/crates/turbo-tasks-env/src/dotenv.rs (2) — F-032
- [x] turbopack/crates/turbo-tasks-backend/src/database/turbo/mod.rs (2) — F-022
- [x] turbopack/crates/turbo-tasks-backend/src/backend/storage_schema.rs (2) — sound (ValueTypeId::new_unchecked(1) is constant)
- [x] turbopack/crates/turbo-tasks-auto-hash-map/src/map.rs (2) — F-046
- [x] turbopack/crates/turbo-persistence/src/shared_bytes.rs (2) — sound (unsafe fn trait contract)
- [x] turbopack/crates/turbo-persistence/src/compression.rs (2) — F-047
- [x] turbopack/crates/turbo-bincode/src/macro_helpers.rs (2) — F-028
- [x] turbopack/crates/turbo-bincode/src/lib.rs (2) — sound (bounds-checked copy_nonoverlapping)
- [x] crates/next-napi-bindings/src/turbopack.rs (2) — F-039
- [x] crates/next-api/src/versioned_content_map.rs (2) — F-031
- [x] turbopack/crates/turbopack-trace-utils/src/trace_writer.rs (1) — F-034
- [x] turbopack/crates/turbopack-trace-server/src/span_ref.rs (1) — F-035
- [x] turbopack/crates/turbopack-trace-server/src/span_graph_ref.rs (1) — F-035
- [x] turbopack/crates/turbopack-trace-server/src/span_bottom_up_ref.rs (1) — F-035
- [x] turbopack/crates/turbopack-trace-server/src/reader/turbopack.rs (1) — F-036
- [x] turbopack/crates/turbopack-ecmascript/src/utils.rs (1) — F-041
- [x] turbopack/crates/turbopack-ecmascript/src/tree_shake/graph.rs (1) — F-044
- [x] turbopack/crates/turbopack-ecmascript/src/references/mod.rs (1) — F-041
- [x] turbopack/crates/turbopack-ecmascript/src/minify.rs (1) — F-044
- [x] turbopack/crates/turbopack-dev-server/src/update/stream.rs (1) — F-041
- [x] turbopack/crates/turbopack-core/src/resolve/alias_map.rs (1) — F-041
- [x] turbopack/crates/turbopack-core/src/module_graph/traced_di_graph.rs (1) — F-041
- [x] turbopack/crates/turbopack-core/src/module_graph/mod.rs (1) — F-041
- [x] turbopack/crates/turbopack-core/src/module_graph/chunk_group_info.rs (1) — F-041
- [x] turbopack/crates/turbo-tasks/src/task/function.rs (1) — sound (test-only macro example)
- [x] turbopack/crates/turbo-tasks/src/read_ref.rs (1) — F-049
- [x] turbopack/crates/turbo-tasks/src/priority_runner.rs (1) — F-048
- [x] turbopack/crates/turbo-tasks/src/local_task_tracker.rs (1) — F-023
- [x] turbopack/crates/turbo-tasks/src/graph/visit.rs (1) — F-050
- [x] turbopack/crates/turbo-tasks/src/graph/adjacency_map.rs (1) — F-041
- [x] turbopack/crates/turbo-tasks-macros/src/value_macro.rs (1) — F-029
- [x] turbopack/crates/turbo-tasks-macros/src/derive/operation_value_macro.rs (1) — F-029
- [x] turbopack/crates/turbo-tasks-macros/src/derive/non_local_value_macro.rs (1) — F-029
- [x] turbopack/crates/turbo-tasks-macros-tests/tests/trybuild.rs (1) — F-032
- [x] turbopack/crates/turbo-tasks-hash/src/base38.rs (1) — F-044
- [x] turbopack/crates/turbo-tasks-fs/src/rope.rs (1) — Note (set_len before read; sound under bincode reader)
- [x] turbopack/crates/turbo-tasks-backend/src/database/write_batch.rs (1) — F-022
- [x] turbopack/crates/turbo-tasks-backend/src/database/noop_kv.rs (1) — F-022
- [x] turbopack/crates/turbo-persistence/src/tests.rs (1) — F-022
- [x] turbopack/crates/turbo-persistence/src/db.rs (1) — F-018
- [x] turbopack/crates/turbo-persistence/src/bin/sst_inspect.rs (1) — F-018
- [x] rspack/crates/binding/src/lib.rs (1) — F-040
