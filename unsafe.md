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
