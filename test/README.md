# Test Instructions for AI Agents

This document provides guidelines for AI agents when writing and executing tests for the eMterm project.

## Test Framework

**Rust Tests:**
- Framework: Rust's built-in testing framework (`cargo test`)
- Additional tools:
  - `cargo-tarpaulin` for code coverage
  - `criterion` for benchmarking (if needed)

**TypeScript Tests:**
- Framework: Bun's built-in test runner
- Test files: `*.test.ts`

## Test Execution

### Unit Tests

**Rust (backend):**
```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

**TypeScript (frontend):**
```bash
bun test
```

### Integration Tests

**Rust integration tests:**
```bash
cargo test --manifest-path src-tauri/Cargo.toml --test '*'
```

**With coverage:**
```bash
cargo tarpaulin --manifest-path src-tauri/Cargo.toml --out Html
```

### E2E Tests

E2E tests require the eMterm application to be running. Use the chrome-devtools MCP skill for browser-based E2E testing.

```bash
# Start eMterm in dev mode
bun tauri dev

# Run E2E tests (in another terminal)
# Use /e2e-testing skill
```

### Type Checking

```bash
bun run typecheck
```

## Test File Organization

### Rust Tests

**Unit tests:**
- Location: Same file as the code being tested (in `#[cfg(test)]` module)
- Example: `src-tauri/src/commands/markdown.rs` contains `mod tests { ... }`

**Integration tests:**
- Location: `src-tauri/tests/integration/`
- File naming: `{feature}_tests.rs` (e.g., `markdown_tests.rs`)
- Test fixtures: `src-tauri/tests/fixtures/`

**Test fixtures structure:**
```
src-tauri/tests/fixtures/
├── markdown/
│   ├── sample.md          # Small valid Markdown
│   ├── large.md           # Near-limit Markdown (close to 2MB)
│   ├── gfm.md             # GFM features (tables, task lists)
│   └── empty.md           # Empty file
└── images/
    ├── small.png          # Small PNG image
    ├── photo.jpg          # JPEG image
    ├── animated.gif       # GIF image
    ├── modern.webp        # WebP image
    ├── large.png          # Near-limit image (close to 10MB)
    ├── tiny.png           # 1x1 pixel image
    └── corrupted.png      # Invalid/corrupted image
```

### TypeScript Tests

**Location:** Alongside source files
- Example: `src/terminal.ts` → `src/terminal.test.ts`

## Writing Tests

### Test Naming Conventions

**Rust:**
```rust
#[test]
fn test_validate_file_path_with_valid_file() { ... }

#[test]
fn test_validate_file_size_exceeds_limit() { ... }

#[test]
fn test_encode_and_chunk_splits_correctly() { ... }
```

**TypeScript:**
```typescript
describe('Terminal', () => {
  test('should render ANSI sequences correctly', () => { ... });

  test('should handle Markdown OSC sequences', () => { ... });
});
```

### Test Structure

**Rust - Table-Driven Tests:**
```rust
#[test]
fn test_validate_file_size() {
    let test_cases = vec![
        ("small file", 1024, 2_000_000, true),
        ("at limit", 2_000_000, 2_000_000, true),
        ("over limit", 2_000_001, 2_000_000, false),
    ];

    for (name, size, limit, should_pass) in test_cases {
        // Create temp file with specified size
        // Run validation
        // Assert result
    }
}
```

**Rust - Standard Unit Test:**
```rust
#[test]
fn test_markdown_osc_generation() {
    // Arrange
    let session_id = Uuid::new_v4();
    let content = "# Hello World";
    let encoded = base64::encode(content);

    // Act
    let result = generate_markdown_osc(&session_id, vec![encoded]);

    // Assert
    assert!(result.contains("emterm;markdown;begin"));
    assert!(result.contains(&session_id.to_string()));
    assert!(result.contains("format=gfm"));
}
```

**TypeScript:**
```typescript
import { describe, test, expect } from 'bun:test';

describe('OscParser', () => {
  test('should parse Markdown OSC sequence', () => {
    // Arrange
    const input = '\x1b]777;emterm;markdown;begin;id=123\x1b\\';

    // Act
    const result = parseOsc(input);

    // Assert
    expect(result.type).toBe('markdown');
    expect(result.verb).toBe('begin');
    expect(result.id).toBe('123');
  });
});
```

## Adding New Tests

### For CLI Commands (Rust)

1. **Add unit tests** in the same file as your implementation:
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_new_feature() {
           // Test implementation
       }
   }
   ```

2. **Add integration tests** in `src-tauri/tests/integration/{feature}_tests.rs`:
   ```rust
   use std::process::Command;

   #[test]
   fn test_markdown_command_with_valid_file() {
       let output = Command::new("emterm")
           .arg("markdown")
           .arg("tests/fixtures/markdown/sample.md")
           .output()
           .expect("Failed to execute command");

       assert!(output.status.success());
       assert!(output.stdout.len() > 0);
   }
   ```

3. **Add test fixtures** if needed in `src-tauri/tests/fixtures/`

### For Frontend (TypeScript)

1. Create `{module}.test.ts` alongside your source file
2. Use Bun's test runner with `describe` and `test` blocks
3. Mock external dependencies when necessary

## E2E Test Guidelines

For E2E tests of the terminal emulator:

1. **Use the e2e-testing skill** - This provides chrome-devtools integration
2. **Test rendering** - Verify that Markdown and images appear correctly
3. **Test interactions** - Verify user interactions work as expected
4. **Cleanup** - Close test windows/pages after tests complete

**Example E2E test workflow:**
```
1. Start eMterm in dev mode
2. Use /e2e-testing skill
3. Navigate to terminal
4. Execute test commands (emterm markdown, emterm image)
5. Verify rendering via screenshots or DOM inspection
6. Cleanup and close
```

## Common Patterns

### Creating Temporary Test Files

```rust
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_with_temp_file() {
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(temp_file, "# Test Content").unwrap();

    // Use temp_file.path() in your test
    let result = execute_markdown_command(temp_file.path());

    assert!(result.is_ok());
}
```

### Capturing stdout/stderr

```rust
use std::process::Command;

#[test]
fn test_error_message() {
    let output = Command::new("emterm")
        .arg("markdown")
        .arg("nonexistent.md")
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("File not found"));
}
```

### Testing OSC Sequence Format

```rust
#[test]
fn test_osc_format() {
    let sequence = generate_markdown_osc(...);

    // Check ESC codes
    assert!(sequence.starts_with("\x1b]777;"));
    assert!(sequence.contains("\x1b\\"));

    // Check parameters
    assert!(sequence.contains("emterm;markdown;begin"));
    assert!(sequence.contains("format=gfm"));
    assert!(sequence.contains("version=1.0"));
}
```

### Mock Data Generators

```rust
// Helper function for tests
fn create_test_markdown_content(size_kb: usize) -> String {
    "# Test\n".repeat(size_kb * 1024 / 7) // Approximate
}

#[test]
fn test_large_markdown() {
    let content = create_test_markdown_content(100); // 100KB
    // Test processing...
}
```

## Coverage Goals

- **Unit test coverage:** ≥ 80% for Rust code
- **Critical paths:** 100% coverage for error handling and validation
- **Integration tests:** All CLI commands and options

## Running Coverage Reports

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --manifest-path src-tauri/Cargo.toml --out Html --output-dir coverage

# View report
open coverage/index.html  # macOS
xdg-open coverage/index.html  # Linux
```

## Performance Benchmarks

For performance-critical code, add benchmarks:

```rust
#[cfg(test)]
mod benchmarks {
    use super::*;
    use std::time::Instant;

    #[test]
    fn bench_markdown_processing() {
        let content = create_test_markdown_content(100);

        let start = Instant::now();
        let _ = process_markdown(&content);
        let duration = start.elapsed();

        assert!(duration.as_millis() < 50, "Processing took too long: {:?}", duration);
    }
}
```

## Continuous Integration

Tests are automatically run on:
- Every commit (pre-commit hook)
- Pull requests
- Main branch merges

**CI Test Commands:**
```bash
# All Rust tests
cargo test --manifest-path src-tauri/Cargo.toml --all

# TypeScript tests
bun test

# Type checking
bun run typecheck

# Linting
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

## Debugging Tests

**Run specific test:**
```bash
cargo test test_name -- --nocapture
```

**Show output:**
```bash
cargo test -- --nocapture --test-threads=1
```

**Run with logging:**
```bash
RUST_LOG=debug cargo test
```

## Best Practices

1. **Test file boundaries:** Always test edge cases (empty files, max size, just over limit)
2. **Test error paths:** Every error condition should have a test
3. **Use descriptive names:** Test names should clearly describe what they test
4. **Keep tests independent:** Each test should be able to run in isolation
5. **Clean up resources:** Use RAII or explicit cleanup for temp files
6. **Mock external dependencies:** Don't rely on network or external services
7. **Document complex tests:** Add comments explaining non-obvious test logic
8. **Fast tests:** Keep unit tests fast (< 100ms each)
9. **Stable tests:** Avoid flaky tests that fail randomly
10. **Meaningful assertions:** Use specific assertions with helpful messages

## Test Checklist for New Features

When implementing a new feature, ensure:

- [ ] Unit tests for all public functions
- [ ] Unit tests for error cases
- [ ] Integration test for happy path
- [ ] Integration test for error conditions
- [ ] Edge case tests (boundary values)
- [ ] Performance test (if applicable)
- [ ] Documentation examples are tested
- [ ] All tests pass locally
- [ ] Code coverage meets threshold
