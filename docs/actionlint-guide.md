# Actionlint Guide

## What is Actionlint?

[Actionlint](https://github.com/rhysd/actionlint) is a static checker for GitHub Actions workflow files. It validates workflow syntax, checks for common mistakes, and helps catch errors before they cause CI/CD failures.

## Why We Use It

- **Early Error Detection**: Catches workflow errors during PR review instead of at runtime
- **Best Practices**: Enforces GitHub Actions best practices
- **Shell Script Validation**: Integrates with shellcheck to validate bash scripts in workflows
- **Type Safety**: Validates expressions, contexts, and action inputs

## How It's Configured

### Workflow Location
`.github/workflows/actionlint.yaml` - Runs on every PR that modifies workflow files

### Configuration File
`.github/actionlint.yaml` - Contains:
- Custom self-hosted runner labels
- Ignore patterns for false positives

### Current Ignore Patterns

```yaml
paths:
  .github/workflows/*.{yml,yaml}:
    ignore:
      - 'default value.*but.*also required'
      - 'property.*is not defined in object type'
```

**Why these patterns?**
- `default value.*but.*also required` - Workflow_call inputs with defaults that are also required (design choice for our reusable workflows)
- `property.*is not defined in object type` - Secret references that actionlint cannot verify (secrets are runtime values)

## Running Actionlint Locally

### Install
```bash
# macOS
brew install actionlint

# Linux
curl -s https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash | bash
```

### Run
```bash
# Check all workflows
actionlint

# Check specific file
actionlint .github/workflows/ci-cloud.yaml

# With verbose output
actionlint -verbose
```

## Common Errors and Fixes

### 1. Shellcheck SC2086: Double quote to prevent globbing

**Error:**
```
shellcheck reported issue in this script: SC2086:info:2:50: Double quote to prevent globbing and word splitting
```

**Cause:** Unquoted variables in shell scripts can cause word splitting or globbing.

**Fix:**
```yaml
# Before
run: echo "value=$MY_VAR" >> $GITHUB_OUTPUT

# After
run: echo "value=$MY_VAR" >> "$GITHUB_OUTPUT"
```

### 2. Unknown Runner Label

**Error:**
```
label "ubuntu-latest-8-core-x64" is unknown
```

**Cause:** Custom self-hosted runner labels need to be declared.

**Fix:** Add to `.github/actionlint.yaml`:
```yaml
self-hosted-runner:
  labels:
    - ubuntu-latest-8-core-x64
    - ubuntu-latest-16-core-x64
```

### 3. Property Not Defined in Object Type

**Error:**
```
property "my_secret" is not defined in object type {github_token: string}
```

**Cause:** Actionlint cannot verify secrets that are defined in repository settings.

**Fix:** Add ignore pattern to `.github/actionlint.yaml`:
```yaml
paths:
  .github/workflows/*.{yml,yaml}:
    ignore:
      - 'property.*is not defined in object type'
```

### 4. Workflow_call Input with Default and Required

**Error:**
```
input "target_env" has the default value "qanet", but it is also required
```

**Cause:** Design pattern where we want a default but also want to force callers to be explicit.

**Fix:** Either:
- Remove `required: true` if default is acceptable
- Remove `default:` if input must always be provided
- Or add ignore pattern if this is intentional

### 5. Expression Syntax Errors

**Error:**
```
unexpected character ')' while lexing expression
```

**Cause:** Invalid GitHub Actions expression syntax.

**Fix:**
```yaml
# Before - missing space
if: ${{github.event_name == 'push'}}

# After - proper spacing
if: ${{ github.event_name == 'push' }}
```

### 6. Missing Required Input

**Error:**
```
missing required input "version" for action "actions/setup-node@v3"
```

**Fix:**
```yaml
- uses: actions/setup-node@v3
  with:
    node-version: '18'  # Add required input
```

## Adding New Ignore Patterns

If you encounter a legitimate false positive:

1. **Verify it's truly a false positive** - Run the workflow manually to confirm it works

2. **Add pattern to `.github/actionlint.yaml`**:
```yaml
paths:
  .github/workflows/*.{yml,yaml}:
    ignore:
      - 'your regex pattern here'
```

3. **Test locally**:
```bash
actionlint .github/workflows/your-workflow.yaml
```

4. **Document why** - Add a comment explaining the ignore pattern

## Regex Pattern Tips

Actionlint uses [RE2 regex syntax](https://github.com/google/re2/wiki/Syntax):

- `.` matches any character
- `.*` matches zero or more of any character
- `\\.` matches literal dot
- `".*"` matches anything in quotes
- Use `.*` not `.+` for more flexible matching

**Examples:**
```yaml
ignore:
  - 'property ".*" is not defined'        # Any property name
  - 'label "ubuntu-.*" is unknown'        # Any ubuntu label
  - 'SC2086:.*'                           # All SC2086 shellcheck warnings
```

## CI Behavior

- **On PR**: Actionlint runs automatically when workflow files are modified
- **Fail on Error**: CI fails if actionlint finds issues (`fail-on-error: true`)
- **Annotations**: Errors appear as GitHub PR annotations at the exact line
- **Ignored Errors**: Filtered errors still appear in verbose logs but don't fail CI

## Troubleshooting

### CI Fails But Local Run Passes

**Possible causes:**
1. Different actionlint versions - Check CI uses same version as local
2. Config file not committed - Ensure `.github/actionlint.yaml` is in git
3. Cached results - Clear GitHub Actions cache

### Ignore Pattern Not Working

**Debugging steps:**
1. Check pattern syntax - Use RE2 regex, not PCRE
2. Test locally: `actionlint -ignore 'your pattern' workflow.yaml`
3. Check exact error message - Pattern must match the full error text
4. Try broader pattern - Use `.*` wildcards

### Too Many False Positives

Consider:
1. Update actionlint version (may have bug fixes)
2. Use path-specific ignore patterns instead of global
3. Fix the root cause instead of ignoring

## Resources

- [Actionlint Documentation](https://github.com/rhysd/actionlint)
- [RE2 Regex Syntax](https://github.com/google/re2/wiki/Syntax)
- [GitHub Actions Expression Syntax](https://docs.github.com/en/actions/learn-github-actions/expressions)
- [Shellcheck Wiki](https://www.shellcheck.net/wiki/)

## Getting Help

If you're stuck:
1. Check this guide for common errors
2. Run with `-verbose` flag to see detailed output
3. Search [actionlint issues](https://github.com/rhysd/actionlint/issues)
4. Ask in team chat with the error message and workflow snippet
