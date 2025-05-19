# Contributing

## Workflow

- We use trunk-based development with short-lived feature branches.
- Follow [Conventional Commits](https://www.conventionalcommits.org/) for commit messages.
- Open a PR for code review and CI checks. We squash-merge PRs into `main`.

## Developer Guidelines

- Write tests for new features or bug fixes.
- Keep documentation up-to-date.
- Run `just all-features` before pushing.

## Issue Reporting

- Use JIRA tickets for bug reports and feature requests.
- Include steps to reproduce, expected behavior, and logs.
- Link issues to epics or stories as needed.

## Code Quality

- Use Clippy (`just lint`) for linting.
- Format code with `rustfmt` (`just +nightly fmt`).
- Add comments and docstrings for public functions and types.
