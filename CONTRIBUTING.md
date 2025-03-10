# Contributing to `interflow`

First off, thank you for considering contributing to this project! It's people like you who make this project better.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How Can I Contribute?](#how-can-i-contribute)
- [Development Process](#development-process)
- [Pull Requests](#pull-requests)
- [Style Guidelines](#style-guidelines)

## Code of Conduct

This project and everyone participating in it is governed by our Code of Conduct. By participating, you are expected to uphold this code.

## How Can I Contribute?

### Reporting Bugs

Before creating bug reports, please check the issue list as you might find out that you don't need to create one. When you are creating a bug report, please include as many details as possible:

* Use a clear and descriptive title
* Describe the exact steps which reproduce the problem
* Provide specific examples to demonstrate the steps
* Describe the behavior you observed after following the steps
* Explain which behavior you expected to see instead and why
* Include any error messages

### Suggesting Enhancements

Enhancement suggestions are tracked as GitHub issues. When creating an enhancement suggestion, please include:

* A clear and descriptive title
* A detailed description of the proposed functionality
* Any possible implementation details
* Why this enhancement would be useful

### Pull Requests

Before starting, try to first open an issue, before starting your work on a PR. If an issue exists, make sure a PR isn't already opened for it.

1. Fork the repository
2. Clone your fork: `git clone https://github.com/solarliner/interflow`
3. Create a new branch: `git checkout -b your-branch-name`. Branch name should begin with `feature/`, `feat/`, `fix/` or `bugfix/` in order to create folders and organize the branches,
   and for this reason, they should be in lowercase (so that we don't end up with `fix` and `Fix` folder on Linux machines!)
4. Make sure to have the [prerequisites](#prerequisites) below
5. Make your changes
6. Run tests: `cargo test`
7. Submit a PR: the title should be descriptive of the changes in one sentence, and should have the `closes \#NNN` message that tells GitHub to automatically close the related
   issue, if there is one (there should, see above).

**Open your PR early!** You can mark your PR as a draft to signal that it isn't yet ready for review. Having PRs opened early shows what is being worked on and reduces
the duplication of effort.

#### Prerequisites

- Rust 1.85.0 or later
- Any supported audio API (or the one you want to add) to be able to run the tests.
- `pre-commit` installed into the repository

#### PR opening process

* Fill in the required template
* Do not include issue numbers in the PR title
* Include screenshots and animated GIFs in your pull request whenever possible
* Follow the Rust style guidelines
* Include tests when adding new features
* Update documentation when changing core functionality

## Development Process

1. Create a new branch from `main`
2. Make your changes
3. Write or update tests as needed
4. Update documentation as needed
5. Run `cargo fmt` to format code
6. Run `cargo clippy` to check for common mistakes
7. Run `cargo test` for unit tests, and run the examples to make sure they still work
8. Commit your changes using clear commit messages (see [Git Commit Message](#git-commit-messages)).
9. Push to your fork
10. Open a Pull Request

## Style Guidelines

### Git Commit Messages

While not enforced, the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) is preferred.

* Use the present tense ("Add feature" not "Added feature")
* Use the imperative mood ("Move cursor to..." not "Moves cursor to...")
* Limit the first line to 72 characters or fewer
* Reference issues and pull requests liberally after the first line

### Rust Code Style

* Follow the [Rust Style Guide](https://doc.rust-lang.org/1.0.0/style/style/README.html)
* Run `cargo fmt` before committing
* Use `cargo clippy` to catch common mistakes
* Write documentation for public APIs
* Include unit tests for new code

## Testing

* Include tests for new features
* Update tests for bug fixes
* CI will ensure tests pass, however it is good practice to ensure all tests pass locally before pushing.
  Make sure to also check out the examples to make sure they work correctly.
* Write both unit tests and integration tests when applicable

## Documentation

* Keep API documentation in code up to date
* Add examples for new features

## Questions?

Feel free to open an issue with your question or contact the maintainers directly.

---

Thank you for contributing to this project! Your time and effort help make this project better for everyone.
