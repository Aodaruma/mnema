# Contributing to Mnema

First of all, thank you for your interest in Mnema.  
This project is still early and experimental, so the contribution process is intentionally light.

## Code of Conduct

Be kind, be respectful, and assume good intent.  
If you are unsure whether something is appropriate, err on the side of being considerate.

A formal Code of Conduct may be added later. Until then, use common sense and basic professionalism.

## Project overview

Mnema is:

- a **local-first**, **AI-assisted** task memory
- written primarily in **Rust**
- targeting desktop first (Windows initially), with a future path to other platforms

For more details about the current design, see:

- `docs/specification-v0.1.md`

## How to contribute

### 1. Discuss before large changes

If you plan to work on a non-trivial feature or refactor, please:

1. Open an issue describing:
   - what you want to change
   - why you think it’s useful
   - any design ideas you already have
2. Wait for some feedback before investing a lot of time.

Small fixes (typos, tiny refactors, etc.) usually don’t need prior discussion.

### 2. Development workflow (draft)

This project is still stabilizing, but a typical flow will look like:

1. Fork the repository and create a feature branch:
   - `feat/...`, `fix/...`, or `chore/...`
2. Make your changes.
3. Run tests / linters if available.
4. Open a pull request:
   - Explain what changed and why.
   - Link to the related issue if there is one.

As the project matures, more concrete instructions (tooling, commands, CI, etc.) will be documented here.

### 3. Style

Because Mnema is early, style rules are intentionally minimal:

- Prefer **clear, intention-revealing names** over cleverness.
- Keep modules small and focused.
- Add comments where the intent is not obvious from the code.
- For UI, prioritize readability and low cognitive load over visual flash.

If you’re unsure, imitate the existing code around your changes.

## License and re-licensing

Mnema is currently licensed under the **Apache License 2.0**.

By contributing to this repository, you agree that:

- Your contributions are licensed under the Apache License 2.0, and
- Your contributions **may be re-licensed in the future under equivalent or more permissive open-source licenses**, such as MIT, as long as your original authorship is preserved in the history and/or attribution.

This is to keep Mnema flexible in the long term (for example, if the project later decides to adopt a simpler permissive license) while still respecting contributors.

If you have concerns about this, please open an issue before submitting significant contributions.

## Attribution

Unless you request otherwise, your Git identity (name/handle) will be used as attribution for your contributions via the Git history and pull requests.

Thank you again for helping shape Mnema. 🙏
