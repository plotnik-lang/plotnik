# CLI UX Research

> Consolidated research on building outstanding CLI user experiences.

## Sources

| Resource                                                                                                                                                   | Description                                      |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| [Command Line Interface Guidelines](https://clig.dev/)                                                                                                     | The definitive guide (Prasad, Firshman, Tashian) |
| [12 Factor CLI Apps](https://medium.com/@jdxcode/12-factor-cli-apps-dd3c227a0e46)                                                                          | Heroku/oclif principles                          |
| [Heroku CLI Style Guide](https://devcenter.heroku.com/articles/cli-style-guide)                                                                            | Production-tested patterns                       |
| [Atlassian: 10 Principles for Delightful CLIs](https://www.atlassian.com/blog/it-teams/10-design-principles-for-delightful-clis)                           | Design principles                                |
| [Evil Martians: Progress Displays](https://evilmartians.com/chronicles/cli-ux-best-practices-3-patterns-for-improving-progress-displays)                   | Spinners, bars, status                           |
| [Lucas Costa: UX Patterns](https://lucasfcosta.com/2022/06/01/ux-patterns-cli-tools.html)                                                                  | Practical patterns                               |
| [Thoughtworks: CLI Design](https://www.thoughtworks.com/en-us/insights/blog/engineering-effectiveness/elevate-developer-experiences-cli-design-guidelines) | DX guidelines                                    |
| [BetterCLI.org](https://bettercli.org/)                                                                                                                    | Comprehensive reference                          |

---

## Part 1: Philosophy

### Human-First Design

Modern CLIs should prioritize human usability over machine compatibility. The historical UNIX approach optimized for scripts and terse output—today's CLIs serve humans directly.

> "If a command is going to be used primarily by humans, it should be designed for humans first."

### Composability

Design programs as modular tools with clean interfaces. Standard mechanisms (stdin/stdout/stderr, signals, exit codes, plain text) ensure interoperability. Programs inevitably become parts of larger systems.

### Consistency

Terminal conventions are hardwired into users' fingers. Following existing patterns makes CLIs intuitive and guessable. Breaking conventions requires careful deliberation.

### Discoverability

CLIs needn't force users to memorize everything. Comprehensive help, examples, error suggestions, and progressive disclosure bridge the gap between CLI efficiency and GUI discoverability.

### Robustness

Software should handle unexpected input gracefully and _feel_ solid—immediate, responsive, reliable. This comes from attention to detail: keeping users informed, explaining errors clearly, avoiding scary stack traces.

### Empathy

Show users you're on their side. Exceed expectations through thoughtful design that demonstrates you've considered their problems carefully.

---

## Part 2: Help & Documentation

### The Help Flag

These should all work equivalently:

```
myapp
myapp --help
myapp -h
myapp help
myapp help subcommand
myapp subcommand --help
```

Adding `-h` to any invocation should display help, ignoring other flags.

### Concise Default Help

When required arguments are missing, show brief help:

- Program description
- One or two example invocations
- Flag descriptions (unless numerous)
- Instructions to pass `--help` for full details

### Lead With Examples

Users prefer examples over other documentation forms. Show common and complex use cases first, building understanding progressively.

```
USAGE
  myapp <command> [options]

EXAMPLES
  $ myapp init                    # Start a new project
  $ myapp build --release         # Build for production
  $ myapp deploy --env staging    # Deploy to staging

COMMANDS
  init      Initialize new project
  build     Compile the application
  deploy    Deploy to environment

Run 'myapp <command> --help' for details.
```

### Organize by Frequency

Display common flags and commands first. Git shows "common commands used in various situations" upfront.

### Include Links

Add a website or GitHub link for feedback/issues. Link to web docs for subcommands when available.

---

## Part 3: Output Design

### Human-Readable First

Optimize output for human consumption by default. Detect TTY to determine if output is interactive.

### Machine-Readable Secondary

- Keep output line-based for `grep`/`awk` compatibility
- Provide `--plain` to disable human formatting
- Provide `--json` for structured data (works with `jq`)

### Success Output

Display concise confirmation on success. Users need feedback, especially for long operations.

```
✓ Configuration saved to ~/.config/myapp/config.toml
```

Provide `-q`/`--quiet` for silent mode.

### State Changes

When modifying state, explain what happened:

```
Compiled 42 files in 1.2s
  → target/release/myapp (2.3 MB)
```

### Suggest Next Steps

In multi-step workflows, suggest commands:

```
Project initialized!

Next steps:
  cd my-project
  myapp build
  myapp run
```

### Tables

Use tables for structured data. Never use borders—they're noisy and break parsing:

**Bad:**

```
+--------+----------+
| Name   | Status   |
+--------+----------+
| foo    | active   |
+--------+----------+
```

**Good:**

```
NAME    STATUS
foo     active
bar     pending
```

---

## Part 4: Color & Visual Design

### Intentional Color

Use color to highlight important text and indicate status—not to decorate everything.

| Color     | Use For                |
| --------- | ---------------------- |
| Red       | Errors                 |
| Yellow    | Warnings               |
| Green     | Success, confirmations |
| Cyan/Blue | Info, emphasis         |
| Magenta   | Special items          |
| Gray/Dim  | Secondary info         |

### Respect NO_COLOR

Disable colors when:

- stdout/stderr isn't a TTY
- `NO_COLOR` environment variable is set
- `TERM=dumb`
- User passes `--no-color`

### Symbols & Emoji

Use sparingly to capture attention or add character:

```
✓ Build succeeded
✗ Tests failed
⚠ Deprecated API used
```

Avoid becoming cartoonish.

### Hide Developer Output

Don't display debug output, log levels, or stack traces by default. Reserve for `-v`/`--verbose` or `--debug`.

---

## Part 5: Error Messages

### The Golden Rules

1. **Human-readable**: Plain language, no jargon
2. **Actionable**: What went wrong + how to fix
3. **Context-aware**: Show relevant values
4. **Searchable**: Unique IDs, "SEO for errors"

### Structure

```
error: Unable to read configuration file

  File: ~/.config/myapp/config.toml
  Cause: Permission denied

Try: chmod +r ~/.config/myapp/config.toml

For help: https://myapp.dev/docs/errors#E0042
```

### Formatting

- Start with lowercase, no period at end
- Position critical info at the end (where eyes rest)
- Use red sparingly
- Group similar errors under one header

### Suggestions

When users mistype, suggest corrections politely:

```
error: Unknown command 'buidl'

Did you mean 'build'?
```

Don't auto-correct without confirmation—users may have made logical, not typographical, errors.

### Exit Codes

- `0`: Success
- Non-zero: Failure
- Map codes to failure modes for script detection

---

## Part 6: Progress & Feedback

### The 100ms Rule

Print something within 100ms, even if it's just a spinner. Responsiveness makes programs feel solid.

### Three Patterns

#### 1. Spinners (Uncertain Duration)

```
⠋ Connecting to server...
```

Best for brief, sequential tasks. Update when specific actions complete.

#### 2. X of Y (Measurable Progress)

```
Downloading: 42/100 files
```

Immediately signals stalls when numbers stop climbing.

#### 3. Progress Bars (Multiple Tasks)

```
Building  [████████░░░░░░░░] 50%
```

Best for lengthy parallel processes.

### Verb Tenses

- Ongoing: gerund form ("Downloading...", "Building...")
- Complete: past tense ("Downloaded", "Built")

```
⠋ Downloading dependencies...
✓ Downloaded 42 packages
⠋ Building project...
✓ Built in 3.2s
```

### Long Operations

For operations taking more than a few seconds:

- Show animated indicator (proves program isn't frozen)
- Display what's happening
- Estimate completion time if possible

### Disable When Not TTY

Prevent progress bars from creating "Christmas trees" in CI logs.

---

## Part 7: Arguments & Flags

### Prefer Flags to Arguments

Flags are more explicit and flexible:

```bash
# Unclear: which is source, which is dest?
myapp copy file1.txt file2.txt

# Clear
myapp copy --from file1.txt --to file2.txt
```

Exception: common actions where brevity is worth memorizing (`cp src dest`).

### Full-Length Versions

Always provide long forms (`--help` alongside `-h`). Scripts benefit from clarity; users benefit from discoverability.

### Standard Names

| Flag            | Meaning                    |
| --------------- | -------------------------- |
| `-h, --help`    | Show help                  |
| `--version`     | Show version               |
| `-v, --verbose` | Verbose output             |
| `-q, --quiet`   | Suppress output            |
| `-f, --force`   | Skip confirmations         |
| `-n, --dry-run` | Simulate without executing |
| `-o, --output`  | Output file                |
| `--json`        | JSON output                |
| `--no-color`    | Disable colors             |
| `--no-input`    | Disable prompts            |

### Never Read Secrets From Flags

Flags leak into `ps` output and shell history. Use `--password-file` or stdin.

---

## Part 8: Interactivity

### Check for TTY

Only use prompts when stdin is interactive. This reliably distinguishes piped/scripted usage.

### Never Require Prompts

Always provide non-interactive alternatives via flags. When stdin isn't a TTY, skip prompts and require explicit flags.

### Confirm Dangerous Actions

| Danger Level | Approach                       |
| ------------ | ------------------------------ |
| Mild         | Optional confirmation          |
| Moderate     | Prompt; offer `--dry-run`      |
| Severe       | Require typing the target name |

For scripts, accept `-f`/`--force` or `--confirm="name"`.

### Escape Mechanisms

- Ctrl-C should always work
- Make exit methods clear
- On second Ctrl-C, skip cleanup

---

## Part 9: Subcommands

### Structure

For complex tools, use subcommands: `myapp <command> [options]`

### Consistency

Use identical conventions across all subcommands:

- Same flag names
- Same output formatting
- Same error style

### Naming

Use `noun verb` (more common) or `verb noun`—be consistent:

```
myapp config get
myapp config set
myapp user list
myapp user create
```

### Avoid Ambiguity

Don't use similar names like "update" and "upgrade" interchangeably.

---

## Part 10: Configuration

### Precedence (High to Low)

1. Flags (highest priority)
2. Environment variables
3. Project config (`.myapprc`, `myapp.toml`)
4. User config (`~/.config/myapp/`)
5. System config

### XDG Specification

Use `~/.config/` to reduce dotfile proliferation. Standard adopted by fish, neovim, tmux, and others.

### Environment Variables

| Variable   | Purpose               |
| ---------- | --------------------- |
| `NO_COLOR` | Disable colors        |
| `DEBUG`    | Verbose output        |
| `EDITOR`   | For editing prompts   |
| `PAGER`    | Output pagination     |
| `TERM`     | Terminal capabilities |

### Secrets

Never store in environment variables—too prone to leakage. Use credential files, pipes, or secret managers.

---

## Part 11: Robustness

### Validate Early

Check input immediately. Provide clear, actionable errors before problems cascade.

### Timeouts

Configure reasonable defaults so programs don't hang on failed connections.

### Crash-Only Design

Build programs that can exit immediately without cleanup. Defer cleanup to next run.

### Plan for Misuse

Users will:

- Wrap your program in scripts
- Use poor network connections
- Run multiple instances simultaneously
- Use unexpected environments

Design defensively.

---

## Part 12: Future-Proofing

### Keep Changes Additive

Add new flags; don't modify existing ones. Avoid breaking changes.

### Deprecation

Warn users of upcoming changes:

```
warning: --old-flag is deprecated, use --new-flag instead
         Will be removed in v3.0
```

### No Arbitrary Abbreviations

Don't allow prefix matching (`i` for `install`). Explicit aliases are fine, but arbitrary abbreviations prevent future expansion.

---

## Quick Reference Card

### Do

- `-h`/`--help` everywhere
- Exit 0 on success, non-zero on failure
- Errors to stderr, output to stdout
- Support `--json` for scripting
- Respect `NO_COLOR`
- Show progress for >2 second operations
- Suggest corrections for typos
- Provide examples in help

### Don't

- Require interactive prompts
- Read secrets from flags
- Print debug output by default
- Break grep/jq compatibility
- Force colors in pipes
- Leave users waiting without feedback
- Use similar command names (update/upgrade)
- Phone home without consent

---

## Rust Libraries

| Library                                              | Purpose             |
| ---------------------------------------------------- | ------------------- |
| [clap](https://github.com/clap-rs/clap)              | Argument parsing    |
| [ratatui](https://github.com/ratatui-org/ratatui)    | TUI framework       |
| [indicatif](https://github.com/console-rs/indicatif) | Progress bars       |
| [console](https://github.com/console-rs/console)     | Terminal styling    |
| [dialoguer](https://github.com/console-rs/dialoguer) | Interactive prompts |
| [colored](https://github.com/colored-rs/colored)     | Colored output      |
| [anyhow](https://github.com/dtolnay/anyhow)          | Error handling      |
| [miette](https://github.com/zkat/miette)             | Fancy diagnostics   |

---

## Plotnik-Specific Notes

### Current Strengths

- Diagnostics system already provides user-friendly errors
- Multiple output formats (`--cst`, `--raw`, `--types`, `--bytecode`)

### Opportunities

1. **Examples in help**: Add usage examples to each subcommand
2. **Suggest corrections**: For misspelled node kinds or capture names
3. **Progress indication**: For large file processing
4. **Exit codes**: Semantic codes for different error types
5. **JSON output**: `--json` flag for machine consumption
6. **Quiet mode**: `-q` for silent operation
7. **Color semantics**: Consistent color usage across diagnostics
