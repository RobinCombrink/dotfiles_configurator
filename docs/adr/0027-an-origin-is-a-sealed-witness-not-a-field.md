# An origin is a sealed witness, not a field

Status: accepted (2026-08-10, grilling session on the settings a configuration owns)

Once a resource carries the settings of the configuration it came from, the thing that must hold is
that it is only ever processed with *its own*. That is a fact about where a value came from, not
about what it is: a symlink is the same symlink whichever configuration declared it, so there is no
type-level dependency to encode. What is wanted is a dependent pair — a resource indexed by its
origin — and the encoding Rust has for one with a runtime index is an existential package.

So the pair is a struct with private fields whose only constructor is a method on the resolved
configuration, and the origin inside it is never exposed. There is no accessor. A caller gets the
answers an origin produces — the files root, the account, the resource's identity, its requirements
— and never the origin itself, so no loose origin exists anywhere to be paired with the wrong
resource. Those methods are the eliminators of the package: it can be observed only through
operations that apply the witness correctly.

This is the same shape as any invariant Rust's types cannot state: a small module with private
fields where the invariant is discharged once, and everything outside it safe by construction.

## Considered options

- **A pair struct with public fields and a careful constructor.** The resource is carried by value
  into six further structs on the way through assessment, the change set and the apply outcome. Each
  would hold an origin and a resource as siblings, so there are seven places a wrong origin
  type-checks rather than one.
- **A generative brand** — an invariant lifetime per configuration, so an origin and a resource can
  meet only when they share it. This is the technique that sounds like the answer and structurally
  cannot be: a collection admits one brand, and a desired state holds resources from several
  configurations by definition. Storing them together requires erasing the brand per element, and
  the erasure is where the guarantee would have lived. Branding answers "this index belongs to that
  slice", where one witness covers the whole collection; it cannot answer "each of these belongs to
  a different one of many scopes, all held together".
- **Running every stage per configuration**, so the origin is the scope rather than a value and the
  pair never exists at all. It reaches the same guarantee, and pays for it by reshaping the change
  set and the apply outcome into grouped forms and moving change-set output from a single ordering
  by kind to one ordering per configuration.

## Consequences

- **There is no accessor for an origin, and adding one ends the guarantee** without any test
  failing. That is what this record exists to prevent.
- **Reading a machine no longer answers where repositories or dotfiles live.** Both were global
  answers on the machine and are now per-resource, so the two methods leave the trait and the
  sealed package answers instead.
- **A resource's identity and its requirements move off the resource.** Identity is the directory a
  clone claims and a requirement names the clone a symlink waits for; neither is answerable without
  the origin.
- **The declared and resolved forms are distinct types**, so a change set holding a resource with no
  settings attached does not compile — which was the point of separating them.
- **The boundary is auditable rather than proven.** Mispairing inside the module still type-checks;
  what the design buys is that there is exactly one place to audit, and it is the place that already
  has to be correct.
