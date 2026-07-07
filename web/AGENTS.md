## Playground architecture

Product intent and roadmap: `docs/wip/playground-design.md` (read it before
adding playground features). `src/components/playground/` is layered; put new
code in the right layer and keep the arrows pointing one way
(wire → engine → orchestration → shell → panes):

| Layer         | Files                                                                     | Rule                                                                                                                        |
| ------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Wire          | `protocol.ts`                                                             | Hand-written mirror of `crates/plotnik-wasm/src/wire.rs`; byte offsets, converted via `byte-offsets.ts` at the point of use |
| Engine        | `worker.ts`, `client.ts`                                                  | Dumb pass-through of the wasm surface; no policy                                                                            |
| Orchestration | `use-session.ts`                                                          | The only caller of the client: debounce, latest-wins, stale-while-error                                                     |
| Shell         | `Playground.tsx`                                                          | User inputs, layout, and the two mediator duties: editor feedback + cross-pane hover routing (never pane-to-pane)           |
| Panes         | `ast-view.tsx`, `output-panel.tsx`, `code-editor.tsx`                     | Presentation over session state; no async, no client                                                                        |
| Interactions  | `spotlight.ts` + adapters `cm-spotlight.ts`, `ast-dom.ts`; `ast-index.ts` | `spotlight.ts` is pane-agnostic; adapters translate pane geometry into it; `ast-index.ts` stays DOM-free                    |

## Playground wasm

The playground imports generated bindings from `src/lib/plotnik-wasm/`
(gitignored). On a fresh checkout, or after changing `crates/plotnik-wasm`,
regenerate them before `astro dev`/`astro build`:

```
make -C .. wasm-web
```

Requires the wasm toolchain from the root Makefile (Homebrew LLVM +
`wasm-bindgen-cli` matching the `wasm-bindgen` version in `Cargo.lock`).

## Development

When starting the dev server, use background mode:

```
astro dev --background
```

Manage the background server with `astro dev stop`, `astro dev status`, and `astro dev logs`.

## Documentation

Full documentation: https://docs.astro.build

Consult these guides before working on related tasks:

- [Adding pages, dynamic routes, or middleware](https://docs.astro.build/en/guides/routing/)
- [Working with Astro components](https://docs.astro.build/en/basics/astro-components/)
- [Using React, Vue, Svelte, or other framework components](https://docs.astro.build/en/guides/framework-components/)
- [Adding or managing content](https://docs.astro.build/en/guides/content-collections/)
- [Adding styles or using Tailwind](https://docs.astro.build/en/guides/styling/)
- [Supporting multiple languages](https://docs.astro.build/en/guides/internationalization/)
