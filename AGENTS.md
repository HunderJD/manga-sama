# AGENTS.md

How to work on manga-sama, for AI coding agents and for humans. Read [AI-POLICY.md](AI-POLICY.md) too.

## The one rule: small steps

The owner reads every line. A change he can't follow is a change he can't accept.

- **One change, one purpose.** Adding a feature doesn't rewrite what's around it. If something has to change first (a function signature, how a loader works), that's its own commit, done before, with nothing else in it.
- **Change the least code that works.** Don't touch a file the change doesn't need. No renames, reformatting or "while I'm here" cleanups mixed in.
- **Keep the existing shape.** Follow the patterns already in the code before inventing new ones. A new pattern needs a reason you can say in one sentence.
- **If a step gets big, stop and split it.** A commit someone can review in five minutes is the target. A pull request that touches most of the code is a sign the work wasn't split, not a sign it was needed.
- **Stay in your scope.** A task touches only the files it needs. If you think you found a bug elsewhere, or you're sure of it, say so (where, why), but don't fix it in the same change: it gets its own task.
- **Explain before you build.** For anything bigger than a fix, say what you'll change and why, and wait for a yes.

## Code: KISS

Keep it simple, stupid. This is a small app: the code should read like a small app.
If a beginner in Rust can't follow a function, it's too complex, whatever it does.

- **Simple over clever.** Plain functions, plain loops, plain types. No abstraction until a second real use exists (the `Source` trait came with the second site, not before).
- **No code without a reason.** No function, `if`, error catch or cast that doesn't do something real. No speculative options, no "for later".
- **Few comments, and only for why.** Most code needs none. Write one only when the reason isn't in the code: a site's odd format, a terminal limit, a trap. A comment that repeats the line under it, narrates the obvious or pads a doc gets deleted.
- **Names say what things are.** No single letters outside tiny closures, no reused name for two meanings.
- **Dependencies are a last resort.** Say what a new crate costs (build time, transitive crates, system libraries) before adding it.
- **Texts go through `t()`.** Every user-facing text is a key in both `locales/en.json` and `locales/fr.json`.

### Where things live

- `src/main.rs`: chains the screens (search → versions → chapters → reader).
- One file per screen; `*_loader.rs` files are background threads that draw nothing.
- `src/sources/`: one module per website, each implementing `Source`. Adding a site is one module and one line in `SOURCES`.
- `src/kitty.rs`: the kitty graphics protocol, written by hand.

### Websites

- Use what the site's own pages use (its search bar, its reader). Never get around a block (Cloudflare challenge, login, paywall): report it and stop.
- Be polite: one request at a time, an honest User-Agent (`manga-sama/<version>`).
- Test parsers on real markup copied from the site, trimmed, with real URLs. No network in tests.

## Checks before a commit

```sh
cargo fmt
cargo clippy --all-targets   # no warnings
cargo test
bash scripts/check-i18n.sh
```

## Git

- Work on a branch from `dev`: `feat/…`, `fix/…`, `docs/…`.
- Conventional commits, one line: `feat(reader): …`, `fix(search): …`. No body, no `Co-Authored-By` line.
- Pull request titles follow the same `type(scope): what` format.
- Commit only what the owner has seen working. Don't commit things you'll take back later.
- Pull requests go to `dev`. `dev` goes to `main` with **Create a merge commit**, never Rebase and merge. Releases are `v*` tags on `main`.
- Never rewrite shared history: no `--amend` on a pushed commit, no force push, no `git checkout --`, `restore`, `stash` or `reset` on someone else's uncommitted work.
- Don't push unless asked.
