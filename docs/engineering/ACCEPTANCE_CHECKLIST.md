# Acceptance Checklist

## Code
- [ ] Public API has docs/comments for non-trivial behavior
- [ ] Error types are explicit and non-stringly typed at boundaries
- [ ] No dead code in production paths

## Tests
- [ ] Positive path tests
- [ ] Constraint violation tests
- [ ] Anti-gaming holdout tests

## Quality Gates
- [ ] cargo fmt --all --check
- [ ] cargo check --all-targets --all-features -q
- [ ] cargo test --all-targets --all-features
- [ ] cargo clippy --all-targets --all-features -- -D warnings
