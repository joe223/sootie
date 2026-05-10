# Commit Message Guidelines

This project uses [Conventional Commits](https://www.conventionalcommits.org/) specification for commit messages, enforced by `cargo-commitlint`.

## Setup

### Quick Setup (Recommended)

Use the bootstrap script to initialize everything automatically:

```bash
./bootstrap.sh
```

This will install cargo-commitlint and set up the Git hook, along with other development tools.

### Manual Setup

#### Install cargo-commitlint

```bash
cargo install cargo-commitlint
```

#### Install Git Hook

The project includes a `.commitlint.yaml` configuration file. To set up the git hook:

```bash
cargo commitlint install
```

This installs a `commit-msg` hook that validates every commit message before it's accepted.

## Commit Message Format

```
<type>[optional scope]: <subject>

[optional body]

[optional footer(s)]
```

### Types

Allowed commit types:

- `feat`: A new feature
- `fix`: A bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, semicolons, etc.)
- `refactor`: Code refactoring without changing functionality
- `perf`: Performance improvements
- `test`: Adding or modifying tests
- `build`: Build system or dependency changes
- `ci`: CI/CD configuration changes
- `chore`: Maintenance tasks
- `revert`: Reverting a previous commit

### Scopes

Scope is optional but recommended for larger changes. Use lowercase:

```
feat(vision): add auth token support
fix(mcp): correct selector resolution
docs(api): update tool reference
```

### Subject

- Use lowercase or sentence-case
- No trailing period
- Imperative mood ("add feature" not "added feature")
- Maximum 100 characters
- Minimum 10 characters

### Examples

#### Valid commits

```bash
feat: add multi-display support
fix(vision): prevent SidecarGuard from killing sidecar process
docs: document Vision backend known issues
chore: add package metadata
refactor(core): simplify target resolution logic
```

#### Invalid commits (will be rejected)

```bash
# Missing type
add new feature

# Invalid type
feature: add new feature

# Trailing period
feat: add new feature.

# Too short
feat: add

# Wrong case for type
Feat: add new feature
```

## Manual Validation

You can manually validate a commit message before committing:

```bash
# Validate a specific message
cargo commitlint check --message "feat: add new feature"

# Validate from stdin
echo "feat: test" | cargo commitlint check

# Validate a file (e.g., git's commit message file)
cargo commitlint check --edit

# Validate recent commits
cargo commitlint check --from HEAD~5 --to HEAD
```

## Configuration

The configuration is stored in `.commitlint.yaml` at the project root. Key rules:

- Header max length: 100 characters
- Header min length: 10 characters
- Body line max length: 100 characters
- Footer line max length: 100 characters
- All standard conventional commit types allowed
- Scope must be lowercase
- Subject must be lowercase or sentence-case

## Bypassing Validation (Emergency Only)

In rare emergency situations, you can bypass the hook:

```bash
git commit --no-verify
```

**Warning:** Use this sparingly. Non-compliant commits break the commit history consistency and make automated tooling (like changelog generation) harder.

## Troubleshooting

### Hook not working

If the hook doesn't run:

1. Verify hook installation:
   ```bash
   ls .git/hooks/commit-msg
   ```

2. Reinstall:
   ```bash
   cargo commitlint uninstall
   cargo commitlint install
   ```

3. Check cargo-commitlint is in PATH:
   ```bash
   which cargo-commitlint
   ```

### Understanding errors

Run the check manually to see detailed errors:

```bash
cargo commitlint check --message "your message here"
```

## Resources

- [Conventional Commits Specification](https://www.conventionalcommits.org/)
- [cargo-commitlint Documentation](https://pegasusheavy.github.io/cargo-commitlint)
- [Angular Commit Conventions](https://github.com/angular/angular/blob/master/CONTRIBUTING.md#commit) (original inspiration)