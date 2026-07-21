# Contributing

## Before you commit

```sh
cargo fmt
cargo clippy --all-targets
cargo test
```

Changes to the serial protocol or reconnect logic cannot be covered by the test
suite — exercise them against a fake device first, as described in `CLAUDE.md`.

## Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>(<optional scope>): <description>

<optional body>

<optional trailers>
```

Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`.
Scopes in this repository are usually the module: `radpro`, `mqtt`, `discovery`,
`cli`.

The description is imperative, lower case, and has no trailing period. A breaking
change is marked with `!` before the colon (`feat(cli)!: rename --node-id`) and
explained in the body.

```
fix(radpro): treat ERROR as an unsupported property, not a link failure

Firmware without deviceBatteryVoltage answered ERROR, which tore down the
serial connection on every poll.
```

## Attribution trailers

**`Co-authored-by:` is reserved for humans.** It denotes a person who shares
authorship of the change, and nothing else. Never use it for a model, an agent, or
a tool — doing so credits software as a person and corrupts the contributor
history.

Work produced with the help of an AI tool carries an `Assisted-by:` trailer
instead, in the Linux kernel style, naming the tool and the exact model version:

```
Assisted-by: <tool>:<model-version>
```

For example:

```
feat(discovery): publish diagnostic entities for tube lifetime

Assisted-by: claude-code:claude-opus-4-8
Signed-off-by: Jane Doe <jane@example.com>
```

Use it whenever a tool made a non-trivial contribution to the patch — generated
code, wrote the tests, drafted the message. Editor completions and spell checking
do not need it. If several tools were involved, add one trailer per tool.

The trailer records assistance, not responsibility: you are the author of every
patch you send, and you are expected to understand and stand behind all of it.
