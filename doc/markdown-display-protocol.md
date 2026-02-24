# Markdown Display Protocol for Terminal Emulators

**Version:** 1.0
**Status:** Draft

## 1. Overview

This specification defines a protocol for displaying rendered Markdown content within terminal emulators. The protocol uses OSC (Operating System Command) escape sequences to transfer Markdown content from a CLI application to the terminal emulator, which then renders it as formatted HTML in a fullscreen overlay.

The protocol is designed to be:

- **Stateless on the sender side** -- the CLI tool outputs escape sequences to stdout and exits. No bidirectional communication is required.
- **SSH-transparent** -- sequences travel through stdout, so they work over SSH without modification.
- **tmux-compatible** -- DCS passthrough wrapping is defined for tmux environments.
- **Chunked** -- large documents are split into fixed-size chunks to avoid exceeding terminal buffer limits.

## 2. Terminology

| Term | Definition |
|------|-----------|
| **Sender** | The process that generates and outputs the escape sequences (e.g., a CLI tool). |
| **Terminal** | The terminal emulator that receives, parses, and renders the Markdown content. |
| **Session** | A single Markdown transfer, identified by a UUID, consisting of a `begin`, one or more `chunk`s, and an `end`. |
| **ST** | String Terminator: `ESC \` (0x1B 0x5C). |
| **OSC** | Operating System Command: `ESC ]` (0x1B 0x5D). |
| **BEL** | Bell character (0x07). MAY be used as an alternative OSC terminator instead of ST, but ST is RECOMMENDED. |

## 3. Sequence Format

All sequences use OSC 777 with the following structure:

```
ESC ] 777 ; emterm ; markdown ; <verb> ; <params...> ST
```

**Byte-level breakdown:**

| Component | Bytes | Description |
|-----------|-------|-------------|
| OSC introducer | `0x1B 0x5D` | `ESC ]` |
| OSC number | `0x37 0x37 0x37` | `777` (ASCII) |
| Separator | `0x3B` | `;` |
| Namespace | `emterm` | Fixed string |
| Separator | `0x3B` | `;` |
| Subsystem | `markdown` | Fixed string |
| Separator | `0x3B` | `;` |
| Verb | ASCII string | `begin`, `chunk`, or `end` |
| Parameters | `;key=value` pairs | Zero or more, each prefixed with `;` |
| ST | `0x1B 0x5C` | `ESC \` |

Parameters are key=value pairs separated by `;`. Keys and values consist of printable ASCII characters. The `=` character separates key from value. The first `=` in each parameter is the delimiter; subsequent `=` characters are part of the value.

## 4. Session Lifecycle

A Markdown display session consists of exactly three phases, executed in order:

```
begin  -->  chunk (1 or more)  -->  end
```

### 4.1 Begin

Starts a new session. The terminal MUST allocate resources for accumulating chunks.

```
ESC ] 777 ; emterm ; markdown ; begin ; id=<uuid> ; format=<format> ; version=<version> ST
```

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `id` | Yes | UUID v4 identifying this session. MUST be unique across concurrent sessions. |
| `format` | No | Markdown format: `gfm` (GitHub Flavored Markdown) or `commonmark`. Default: `commonmark`. |
| `version` | No | Protocol version. Currently `1.0`. Terminals SHOULD ignore sessions with unrecognized major versions. |

The `render` parameter MAY appear for forward compatibility but has no defined values in version 1.0.

**Terminal behavior:**
- If the session `id` already exists, the terminal SHOULD reject the duplicate and log a warning.
- If the maximum number of concurrent sessions is reached, the terminal SHOULD reject the new session.

### 4.2 Chunk

Sends a segment of Base64-encoded Markdown content.

```
ESC ] 777 ; emterm ; markdown ; chunk ; id=<uuid> ; seq=<n> ; data=<base64> ST
```

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `id` | Yes | Session ID, matching the `begin` session. |
| `seq` | Yes | Sequence number, starting from `0`, incrementing by 1 for each chunk. |
| `data` | Yes | Base64-encoded content (RFC 4648 standard alphabet: `A-Z`, `a-z`, `0-9`, `+`, `/`, `=`). |

**Encoding rules:**
1. The original Markdown content is UTF-8 text.
2. The UTF-8 bytes are encoded to Base64 using standard RFC 4648 encoding.
3. The Base64 string is split into chunks at byte boundaries of the Base64 output (not at UTF-8 character boundaries of the original text).
4. Each chunk's `data` value contains one segment of the Base64 string.

**Recommended chunk size:** 128 KB of Base64-encoded data per chunk (approximately 96 KB of decoded content). Terminals MUST support chunks up to at least 256 KB. Senders SHOULD NOT exceed 256 KB per chunk.

**Terminal behavior:**
- The terminal MUST store chunks indexed by `seq` number.
- Chunks MAY arrive out of order (though senders typically send them in order).
- The terminal SHOULD reset the session timeout on each chunk receipt.
- If `id` does not match any active session, the terminal SHOULD discard the chunk and log a warning.

### 4.3 End

Signals that all chunks have been sent. The terminal assembles the content and renders it.

```
ESC ] 777 ; emterm ; markdown ; end ; id=<uuid> ST
```

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `id` | Yes | Session ID, matching the `begin` session. |

**Terminal behavior:**
1. Sort accumulated chunks by `seq` number in ascending order.
2. Concatenate the `data` values to reconstruct the full Base64 string.
3. Decode the Base64 string to obtain UTF-8 bytes.
4. Validate that the bytes are valid UTF-8. If invalid, the terminal SHOULD display an error or fall back to plain text.
5. Parse the UTF-8 text as Markdown (per the `format` specified in `begin`).
6. Sanitize the resulting HTML to prevent XSS (see Section 7).
7. Display the rendered content in a fullscreen overlay.
8. Delete the session and release associated resources.

## 5. Content Encoding

### 5.1 Base64

Content MUST be encoded using standard Base64 (RFC 4648, Section 4):
- Alphabet: `A-Z`, `a-z`, `0-9`, `+`, `/`
- Padding: `=`
- Line breaks within the encoded data MUST NOT be inserted.
- URL-safe Base64 (`-`, `_`) MUST NOT be used.

### 5.2 Chunking

The sender splits the Base64 string into sequential chunks:

```
Original UTF-8 text
  --> UTF-8 bytes
    --> Base64 encode (single string)
      --> Split into chunks of N bytes
        --> Each chunk becomes a `data` parameter value
```

The split occurs on the Base64 string, not on the original UTF-8 bytes. This means a multi-byte UTF-8 character may be split across chunk boundaries at the Base64 level. The terminal MUST concatenate all Base64 chunks before decoding.

### 5.3 No Size Limit

The protocol does not impose a maximum document size. Terminals SHOULD support documents of at least several megabytes. The chunked transfer design allows arbitrarily large documents.

## 6. tmux Passthrough

When the sender is running inside tmux, OSC sequences are consumed by tmux and do not reach the outer terminal. To work around this, each sequence MUST be wrapped in a DCS passthrough envelope.

### 6.1 Detection

The sender detects tmux by checking for the `TMUX` environment variable.

### 6.2 Wrapping Format

```
ESC P tmux; <escaped-sequence> ESC \
```

- `ESC P` (0x1B 0x50): DCS introducer
- `tmux;`: Fixed prefix
- `<escaped-sequence>`: The original OSC sequence with every `ESC` (0x1B) byte doubled to `ESC ESC` (0x1B 0x1B)
- `ESC \` (0x1B 0x5C): ST terminator for the DCS envelope

### 6.3 Per-Sequence Wrapping

When a Markdown session produces multiple OSC sequences (begin + chunks + end), each sequence MUST be wrapped individually in its own DCS passthrough envelope. This prevents exceeding tmux's passthrough buffer limit.

**Correct (each sequence wrapped individually):**
```
ESC P tmux; <escaped-begin-sequence> ESC \
ESC P tmux; <escaped-chunk-0-sequence> ESC \
ESC P tmux; <escaped-chunk-1-sequence> ESC \
ESC P tmux; <escaped-end-sequence> ESC \
```

**Incorrect (all sequences in one envelope):**
```
ESC P tmux; <escaped-begin><escaped-chunk-0><escaped-chunk-1><escaped-end> ESC \
```

### 6.4 tmux Configuration

The user MUST enable passthrough in their tmux configuration:

```
set -g allow-passthrough on
```

## 7. Security

Terminals MUST sanitize rendered HTML to prevent XSS attacks. The Markdown content is untrusted input.

### 7.1 Allowed HTML Elements

Terminals SHOULD allow only the following HTML elements in rendered output:

- **Headings:** `h1` through `h6`
- **Block:** `p`, `br`, `hr`, `div`, `span`
- **Lists:** `ul`, `ol`, `li`
- **Formatting:** `strong`, `b`, `em`, `i`, `del`, `s`, `mark`, `sub`, `sup`
- **Code:** `pre`, `code`, `blockquote`
- **Tables:** `table`, `thead`, `tbody`, `tfoot`, `tr`, `th`, `td`
- **Links:** `a`
- **Images:** `img`
- **Task lists:** `input` (checkbox only)

### 7.2 Forbidden Elements

Terminals MUST strip or reject the following elements:

`script`, `style`, `iframe`, `object`, `embed`, `form`, `base`, `meta`, `link`, `noscript`, `svg`, `math`

### 7.3 Forbidden Attributes

All event handler attributes MUST be removed:

`onclick`, `onerror`, `onload`, `onmouseover`, `onfocus`, `onblur`, `onchange`, `onsubmit`, `onkeydown`, `onkeyup`, `onkeypress`

Additionally: `formaction`, `srcdoc`, `action`, `background`, `dynsrc`, `lowsrc`

### 7.4 URL Sanitization

Permitted URL schemes: `http`, `https`, `mailto`, `tel`, `callto`, `cid`, `xmpp`

`javascript:` and `data:` URIs MUST be rejected.

## 8. Display

### 8.1 Fullscreen Overlay

The terminal displays rendered Markdown in a fullscreen overlay that covers the terminal viewport (similar to the `less` pager). The terminal content beneath the overlay is preserved and restored when the overlay is closed.

### 8.2 Keyboard Navigation

Terminals SHOULD support the following keyboard shortcuts in the overlay:

| Key | Action |
|-----|--------|
| `Escape` or `q` | Close overlay |
| `Arrow Up` / `Arrow Down` | Scroll line by line |
| `Page Up` / `Page Down` | Scroll by page |
| `Home` / `End` | Jump to top / bottom |
| `Ctrl+=` / `Ctrl+-` | Zoom in / out |
| `Ctrl+0` | Reset zoom |

### 8.3 Markdown Features

Terminals SHOULD support:
- CommonMark specification
- GitHub Flavored Markdown (GFM) extensions: tables, task lists, strikethrough, autolinks
- Syntax highlighting for fenced code blocks
- Mermaid diagrams (OPTIONAL)

## 9. Session Management

### 9.1 Timeouts

Terminals SHOULD implement session timeouts to prevent resource leaks from incomplete transfers. Recommended timeout: 30 seconds since the last chunk receipt. The timer SHOULD reset on each chunk, allowing slow or large transfers to complete.

### 9.2 Concurrent Sessions

Terminals SHOULD support at least 10 concurrent sessions. When the limit is reached, new `begin` commands SHOULD be rejected.

### 9.3 Error Handling

- **Unknown session ID in `chunk` or `end`:** Discard silently or log a warning.
- **Invalid Base64 in `chunk`:** Discard the chunk, log a warning. The session MAY continue if subsequent chunks are valid (missing chunks will result in garbled output).
- **Invalid UTF-8 after decoding:** Display an error message or render as escaped plain text.
- **Parse error in Markdown:** Display the raw text as-is in a `<pre>` block.

## 10. Examples

### 10.1 Simple Document

Markdown content: `# Hello World`

Base64 of UTF-8 bytes: `IyBIZWxsbyBXb3JsZA==`

```
ESC ] 777 ; emterm ; markdown ; begin ; id=550e8400-e29b-41d4-a716-446655440000 ; format=gfm ; version=1.0 ESC \
ESC ] 777 ; emterm ; markdown ; chunk ; id=550e8400-e29b-41d4-a716-446655440000 ; seq=0 ; data=IyBIZWxsbyBXb3JsZA== ESC \
ESC ] 777 ; emterm ; markdown ; end ; id=550e8400-e29b-41d4-a716-446655440000 ESC \
```

### 10.2 Multi-Chunk Document

For a document exceeding the chunk size, the Base64 output is split:

```
ESC ] 777 ; emterm ; markdown ; begin ; id=<uuid> ; format=gfm ; version=1.0 ESC \
ESC ] 777 ; emterm ; markdown ; chunk ; id=<uuid> ; seq=0 ; data=<first 128KB of base64> ESC \
ESC ] 777 ; emterm ; markdown ; chunk ; id=<uuid> ; seq=1 ; data=<next 128KB of base64> ESC \
ESC ] 777 ; emterm ; markdown ; chunk ; id=<uuid> ; seq=2 ; data=<remaining base64> ESC \
ESC ] 777 ; emterm ; markdown ; end ; id=<uuid> ESC \
```

### 10.3 tmux Passthrough

The same simple document inside tmux (each ESC in the payload is doubled):

```
ESC P tmux; ESC ESC ] 777 ; emterm ; markdown ; begin ; id=550e8400-e29b-41d4-a716-446655440000 ; format=gfm ; version=1.0 ESC ESC \ ESC \
ESC P tmux; ESC ESC ] 777 ; emterm ; markdown ; chunk ; id=550e8400-e29b-41d4-a716-446655440000 ; seq=0 ; data=IyBIZWxsbyBXb3JsZA== ESC ESC \ ESC \
ESC P tmux; ESC ESC ] 777 ; emterm ; markdown ; end ; id=550e8400-e29b-41d4-a716-446655440000 ESC ESC \ ESC \
```

### 10.4 Sender Implementation (Pseudocode)

```
function send_markdown(file_path):
    content = read_file(file_path)             // UTF-8 text
    session_id = generate_uuid_v4()
    base64_str = base64_encode(content)        // RFC 4648 standard
    chunks = split(base64_str, 128 * 1024)     // 128 KB per chunk

    sequences = ""
    sequences += OSC_777("begin", id=session_id, format="gfm", version="1.0")
    for i, chunk in enumerate(chunks):
        sequences += OSC_777("chunk", id=session_id, seq=i, data=chunk)
    sequences += OSC_777("end", id=session_id)

    if env("TMUX") is set:
        sequences = wrap_each_sequence_for_tmux(sequences)

    write_stdout(sequences)
    flush_stdout()
```

## 11. Design Rationale

### Why OSC 777?

OSC 777 is a commonly used namespace for terminal-specific extensions (e.g., rxvt-unicode uses it for notifications). It avoids conflicts with standardized OSC numbers (0-134) while providing a recognized extension point.

### Why Base64?

OSC sequences are terminated by ST (`ESC \`). Raw Markdown content may contain byte sequences that look like ST or other control characters, corrupting the escape sequence. Base64 encoding guarantees the payload contains only safe ASCII characters.

### Why Chunked Transfer?

- Terminal parsers often have fixed-size buffers for OSC payload data.
- tmux passthrough has a buffer limit (typically 256 KB).
- Chunking allows arbitrarily large documents without requiring terminals to pre-allocate unbounded buffers.

### Why Fullscreen Overlay?

Markdown is a block-level format that does not fit naturally into a character grid. A fullscreen overlay provides proper typography, variable-width fonts, and scrolling, while preserving the underlying terminal state for seamless return.
