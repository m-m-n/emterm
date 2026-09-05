# dompurify 3.4.x heading-loss investigation

## Summary

`dompurify` versions on the 3.4.x line, when run against the `happy-dom`
document the test suite installs (`test-setup.ts`), drop the tag of the
**first top-level element** of any sanitized HTML fragment while keeping
that element's own text content. Against a document whose first block is a
level-1 heading, this manifests as: the `<h1>` wrapper disappears but its
text survives as a bare text node. The same happens for any tag in that
first-element position (`<h2>`, `<div>`, ...) — it is not heading-specific.

**Responsible layer: layer 3 — an interaction between dompurify and the
happy-dom document it operates on** (not a dompurify-only regression, and
not the sanitizer configuration).

The interaction is version-triggered (introduced by a dompurify 3.4.x
internal change) but only manifests against happy-dom's specific
`Node`/`Element` class design, not against a spec-conformant `Node.prototype`
implementation. Neither side is "broken" in isolation; the combination is.

**Adopted version**: `dompurify@^3.4.14`, adoptable with the heading
surviving and sanitizer strictness unchanged, by fixing the test DOM
environment rather than the sanitizer or its configuration (see
"Adoption decision" below).

**Adopted version's license** (read from `node_modules/dompurify/package.json`
at version 3.4.14): `"license": "(MPL-2.0 OR Apache-2.0)"` — a dual grant,
either half of which is compatible with this project's MIT license.

## Reproduction procedure

All steps were run from the repository root with `bun run <script>`, so the
script resolves the project's own `node_modules` (a script run from outside
the project directory resolves bun's global auto-install cache instead, and
silently picks up an unrelated dompurify version — a dead end worth avoiding
when re-running this).

### Step 1 — establish the failing condition (dompurify 3.4.x + happy-dom + the renderer's config)

Environment: `dompurify@3.4.14`, `happy-dom@20.14.0`, this project's
`PURIFY_CONFIG` from `src-tauri/web-shared/markdown/renderer.ts`.

```ts
import { Window } from "happy-dom";
const window = new Window();
globalThis.document = window.document;
globalThis.window = window;

// dynamic import — a top-level `import DOMPurify from "dompurify"` is
// hoisted above the globalThis assignments above and picks up no window.
const { default: DOMPurify } = await import("dompurify");

const rawHtml = "<h1>Title</h1>\n<p>Hello <strong>world</strong>.</p>";
console.log(DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)); // PURIFY_CONFIG copied from renderer.ts
```

Observed output:

```
Title
<p>Hello <strong>world</strong>.</p>
```

The `<h1>` tag is gone; "Title" survives as bare text; the `<p>` is
untouched.

### Step 2 — vary only the dompurify version (config and DOM environment fixed)

Same script, same `PURIFY_CONFIG`, same happy-dom document, only
`dompurify` swapped to `3.3.1` (`bun add dompurify@3.3.1 --exact`).

Observed output:

```
<h1>Title</h1>
<p>Hello <strong>world</strong>.</p>
```

The heading survives intact. This isolates the difference to the dompurify
version change — but see Step 4, which narrows it further to a specific
interaction rather than a self-contained dompurify defect.

### Step 3 — vary only the configuration (dompurify 3.4.14 and DOM environment fixed)

Re-installed `dompurify@3.4.14`. Re-ran the same `rawHtml` with
`PURIFY_CONFIG` replaced by, in turn: no config argument at all (library
defaults), a config with only `ALLOWED_TAGS`/`ALLOWED_ATTR`, and the full
config with `ALLOWED_URI_REGEXP`, `ADD_ATTR`, or `FORBID_TAGS` individually
removed.

Observed output (identical across every variant tried, including "no config
at all"):

```
Title
<p>Hello <strong>world</strong>.</p>
```

The loss persists even with dompurify's own built-in defaults and no
project configuration at all. This rules out the sanitizer configuration
object (layer 2) — the loss is unrelated to any option in `PURIFY_CONFIG`.

### Step 4 — mechanism trace (in place of a second DOM implementation)

No second DOM implementation is available in this project without adding a
new dependency (out of scope for this task), so this step is a source-level
trace rather than an empirical DOM swap, as anticipated by this task's test
notes.

Positional isolation — `DOMPurify.sanitize()` (default config, dompurify
3.4.14, happy-dom) against a range of inputs:

| Input | Output |
|---|---|
| `<h1>Title</h1><p>Body</p>` | `Title<p>Body</p>` |
| `<p>Body</p><h1>Title</h1>` | `Body<h1>Title</h1>` |
| `<h2>Title</h2><p>Body</p>` | `Title<p>Body</p>` |
| `<div>Title</div><p>Body</p>` | `Title<p>Body</p>` |
| `<div><h1>Title</h1><p>Body</p></div>` | `<h1>Title</h1><p>Body</p>` |
| `<h1>One</h1><h1>Two</h1>` | `One<h1>Two</h1>` |

The loss always hits exactly the first top-level element of the sanitized
fragment, regardless of its tag name, and never a later sibling. Wrapping
the same content one level deeper (inside a `<div>`) makes the loss
disappear, because the first top-level element is then the `<div>`, which is
not what breaks — its children are unaffected.

Reading `node_modules/dompurify/dist/purify.es.mjs` (3.4.14) explains why:

- `DOMPurify.sanitize()` parses `dirty` into a document, takes `body = doc.body`,
  and walks `body` and its descendants with a `NodeIterator` rooted at `body`
  (`walkRoot = body`). Per the DOM spec, the first call to
  `nodeIterator.nextNode()` returns the root itself if it passes the
  element filter — so `body` is visited first, exactly as `beforeSanitizeElements`
  hook tracing confirms.
- `body` is not in `ALLOWED_TAGS` by default (only added when
  `WHOLE_DOCUMENT` is set), so `body` is "disallowed" and goes through
  `_sanitizeDisallowedNode`. Because `currentNode === root` there, that
  function *clones* each child and inserts the clones as `body`'s new
  siblings, then force-removes the original `body` (with its original,
  un-cloned children still attached inside it, now detached from the
  document).
- The `NodeIterator` keeps walking into that detached, original `body`'s own
  children next (a node's own children remain reachable locally regardless
  of whether the node itself is attached to a document) — i.e. into the
  *original* `<h1>`, not the newly inserted clone.
- Each element's tag name is read via
  `_readNodeName(node)`, which calls a cached getter obtained by
  `lookupGetter(Node.prototype, 'nodeName')`. In happy-dom
  (`node_modules/happy-dom/lib/nodes/node/Node.js:169`), `Node.prototype`'s
  own `nodeName` getter is an abstract stub — `get nodeName() { return ''; }`
  — meant to be overridden per concrete subclass (e.g.
  `node_modules/happy-dom/lib/nodes/element/Element.js:269`, which returns
  the real tag name). Ordinary property access (`element.nodeName`) resolves
  through the prototype chain to the subclass override and is unaffected.
  dompurify's `lookupGetter` instead reads `Node.prototype`'s *own*
  descriptor directly (a clobbering-resistant caching pattern), which finds
  the stub first and never consults the subclass override.
  Real browsers implement `Node.prototype.nodeName` as a single dispatching
  getter that already returns the correct value for every node type, so this
  divergence has no effect there — the interaction is happy-dom-specific.
- With an empty tag name, `ALLOWED_TAGS['']` is falsy, so the original `<h1>`
  is *also* treated as disallowed and unwrapped (its text moved into the
  detached original `body`, the `<h1>` element force-removed). The `<h1>`
  had no more children, and it is now itself detached (parentless), so the
  `NodeIterator` cannot find a next node and the walk ends there — the `<p>`
  is never visited at all (it happens to still be intact because it was
  never reached, not because it was sanitized).
- The function's final `body.innerHTML` read (`purify.es.mjs`, near the end
  of `DOMPurify.sanitize`) uses the *original*, now-detached `body`
  reference — not the live-tree clones inserted earlier — so the returned
  string reflects whatever is left inside that detached, partially-mutated
  `body`: the (now tag-less) heading text plus whatever untouched siblings
  the aborted walk never reached.

Confirmation: patching happy-dom's `Node.prototype.nodeName` stub, in the
test-only DOM setup, to walk the prototype chain to the actual overriding
subclass getter (matching what ordinary property access already does)
removes the loss entirely, on the unmodified `dompurify@3.4.14`:

```ts
const NodeCtor = window.Node;
const nodeProto = NodeCtor.prototype;
const baseDescriptor = Object.getOwnPropertyDescriptor(nodeProto, "nodeName");
Object.defineProperty(nodeProto, "nodeName", {
  configurable: true,
  get() {
    let proto = Object.getPrototypeOf(this);
    while (proto && proto !== nodeProto) {
      const d = Object.getOwnPropertyDescriptor(proto, "nodeName");
      if (d?.get) return d.get.call(this);
      proto = Object.getPrototypeOf(proto);
    }
    return baseDescriptor.get.call(this);
  },
});
```

With this patch applied before dompurify is imported:

```
<h1>Title</h1>
<p>Hello <strong>world</strong>.</p>
```

This closes the loop: the loss requires *both* dompurify 3.4.x's
clobbering-resistant `nodeName` caching (absent from 3.3.1 — see Step 2) and
happy-dom's stub-plus-subclass-override `nodeName` design (which real
browsers do not share). Neither alone reproduces it, which is why this is
named layer 3 and not layer 1: dompurify's own logic is internally
consistent given a `Node.prototype.nodeName` that is a complete
implementation (true in real browsers), and happy-dom's `nodeName` design is
correct for ordinary property access; only the combination breaks.

## Adoption decision

The newer dompurify line (3.4.x, floor `^3.4.14`) is adopted. Because the
responsible layer is the interaction between dompurify and the test DOM
environment, the fix lives in `test-setup.ts` (the file this project
designates for exactly this outcome), not in `PURIFY_CONFIG` or the
`DOMPurify.sanitize()` call in `renderer.ts` — both are untouched by this
task. The sanitizer's strictness is therefore unchanged: no allowed tag,
allowed attribute, or forbid-entry change, and no URI pattern widening.

`test-setup.ts` now patches happy-dom's `Node.prototype.nodeName` getter (in
the test process only, before any test file runs) to walk the prototype
chain to the actual overriding subclass getter — the value ordinary
`element.nodeName` property access already returns in every environment,
including production's real WebView DOM. This makes the *test* DOM's
observable `nodeName` behavior match what a real browser (and what
`happy-dom`'s own documented per-subclass getters) already provide,
rather than working around the symptom in the sanitizer or its config.

Production (the WebView `renderer.ts` sanitize call, run against a real
browser DOM) never used happy-dom and was not exposed to this interaction at
any dompurify version.

## Reproducing this document's findings

1. From the repository root: `bun install` to materialize `node_modules`.
2. Confirm the currently declared `dompurify` version resolves to a 3.4.x
   release: `grep '"version"' node_modules/dompurify/package.json`.
3. Run `bun test src-tauri/web-shared/markdown/renderer.test.ts` — both
   tests pass, demonstrating the heading survives with the fix in
   `test-setup.ts` in place.
4. To see the failing condition again, temporarily revert the
   `Node.prototype.nodeName` patch block in `test-setup.ts` and re-run the
   same test file — both cases fail with the sanitized heading missing,
   reproducing Step 1 above end-to-end through the actual renderer.
