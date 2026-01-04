# Image Display Feature - Technical Specification

## 1. Overview

This document specifies the technical implementation of inline image display functionality for the eMterm terminal emulator. The implementation supports both Kitty Graphics Protocol and SIXEL protocol, with full support for static images and animations.

### 1.1 Scope

- **In Scope**: Terminal emulator-side image rendering implementation
- **Out of Scope**: CLI `emterm image` command (separate branch)

### 1.2 Design Principles

- Equal priority support for Kitty Graphics Protocol and SIXEL
- Internal storage in RGBA format
- Default transparency rendering below text (negative z-index)
- Image rescaling on window resize
- Non-blocking asynchronous processing to maintain low latency

---

## 2. Supported Protocols Specification

### 2.1 Kitty Graphics Protocol

#### 2.1.1 Sequence Format

The protocol uses APC (Application Programming Command) escape sequences:

```
ESC _ G <control_data> ; <payload> ESC \
```

Where:
- `ESC _` (0x1B 0x5F): APC introducer
- `G`: Graphics command indicator
- `<control_data>`: Comma-separated key=value pairs
- `;`: Separator between control data and payload
- `<payload>`: Base64-encoded binary data
- `ESC \` (0x1B 0x5C): String Terminator (ST)

#### 2.1.2 Actions (`a` key)

| Value | Action | Description |
|-------|--------|-------------|
| `t` | Transmit | Send image data without displaying |
| `T` | Transmit and Display | Send and immediately display image |
| `p` | Put | Display previously transmitted image by ID |
| `q` | Query | Test protocol support |
| `d` | Delete | Remove image(s) from display/memory |
| `f` | Frame | Transmit animation frame |
| `a` | Animate | Control animation playback |
| `c` | Compose | Compose frames together |

#### 2.1.3 Transmission Methods (`t` key)

| Value | Method | Description |
|-------|--------|-------------|
| `d` | Direct | Data embedded in escape sequence (default) |
| `f` | File | Regular file path |
| `t` | Temp File | Temporary file (terminal deletes after read) |
| `s` | Shared Memory | POSIX shared memory object |

#### 2.1.4 Image Format (`f` key)

| Value | Format | Requirements |
|-------|--------|--------------|
| `24` | RGB | Width (`s`) and height (`v`) required |
| `32` | RGBA | Width (`s`) and height (`v`) required (default) |
| `100` | PNG | Dimensions extracted from image data |

#### 2.1.5 Chunked Transfer (`m` key)

For large images, data is split into chunks:

| Value | Meaning |
|-------|---------|
| `1` | More chunks follow |
| `0` | Final chunk (default) |

Maximum chunk size: 4096 bytes (before Base64 encoding).

#### 2.1.6 Compression (`o` key)

| Value | Compression |
|-------|-------------|
| `z` | RFC 1950 ZLIB deflate |

#### 2.1.7 Image Identification

| Key | Description | Range |
|-----|-------------|-------|
| `i` | Image ID | 1 to 4294967295 |
| `I` | Image Number | Non-unique, terminal assigns ID |
| `p` | Placement ID | Unique per image |

#### 2.1.8 Placement Parameters

| Key | Description | Default |
|-----|-------------|---------|
| `c` | Columns to display | Image width in cells |
| `r` | Rows to display | Image height in cells |
| `x` | Source X offset (pixels) | 0 |
| `y` | Source Y offset (pixels) | 0 |
| `w` | Source width (pixels) | Full width |
| `h` | Source height (pixels) | Full height |
| `X` | Cell X pixel offset | 0 |
| `Y` | Cell Y pixel offset | 0 |
| `z` | Z-index (negative = below text) | 0 |
| `C` | Suppress cursor movement (1=yes) | 0 |
| `U` | Unicode placeholder mode (1=yes) | 0 |

#### 2.1.9 Deletion Actions (`d` key with `a=d`)

| Value | Scope | Uppercase Effect |
|-------|-------|------------------|
| `a`/`A` | All visible placements | Free image data |
| `i`/`I` | By image ID | Free image data |
| `c`/`C` | At cursor position | Free image data |
| `p`/`P` | Intersecting cell (x,y) | Free image data |
| `r`/`R` | ID range (x to y) | Free image data |
| `x`/`X` | In column | Free image data |
| `y`/`Y` | In row | Free image data |
| `z`/`Z` | By z-index | Free image data |

#### 2.1.10 Animation Control

**Frame Transmission (`a=f`):**

| Key | Description |
|-----|-------------|
| `Y` | Background RGBA color |
| `c` | Previous frame number for background |
| `X` | Composition mode (1=replace, 0=alpha blend) |
| `x`, `y` | Frame offset in image |
| `s`, `v` | Frame dimensions |
| `z` | Frame gap in milliseconds (negative = gapless) |

**Animation Playback (`a=a`):**

| Key | Description |
|-----|-------------|
| `s=1` | Stop animation |
| `s=2` | Loading mode (wait for frames) |
| `s=3` | Normal loop mode |
| `v` | Loop count (with s=3) |
| `c` | Set current frame |
| `z` | Modify frame timing |

#### 2.1.11 Response Format

**Success:**
```
ESC _ G i=<id> [,p=<placement_id>] ; OK ESC \
```

**Failure:**
```
ESC _ G i=<id> ; ERROR:<message> ESC \
```

**Error Codes:**
- `EINVAL`: Invalid parameters
- `ENOENT`: Image/placement not found
- `ENOSPC`: Storage quota exceeded
- `ECYCLE`: Animation cycle detected
- `ETOODEEP`: Nesting too deep
- `ENOPARENT`: Parent placement not found

**Response Suppression (`q` key):**
- `q=1`: Suppress OK responses
- `q=2`: Suppress ERROR responses

---

### 2.2 SIXEL Protocol

#### 2.2.1 Sequence Format

```
DCS P1 ; P2 ; P3 q <sixel_data> ST
```

Where:
- `DCS`: Device Control String (0x90 or `ESC P` in 7-bit mode)
- `P1`: Pixel aspect ratio parameter
- `P2`: Background color handling
- `P3`: Horizontal grid size (ignored)
- `q`: SIXEL command identifier
- `<sixel_data>`: Encoded sixel characters
- `ST`: String Terminator (0x9C or `ESC \` in 7-bit mode)

#### 2.2.2 Parameters

**P1 - Pixel Aspect Ratio (vertical:horizontal):**

| Value | Ratio |
|-------|-------|
| 0, 1, omitted | 2:1 (default) |
| 2 | 5:1 |
| 3, 4 | 3:1 |
| 5, 6 | 2:1 |
| 7, 8, 9 | 1:1 |

**P2 - Background Handling:**

| Value | Behavior |
|-------|----------|
| 0, 2 | Zero pixels set to background color (default) |
| 1 | Zero pixels retain current color |

#### 2.2.3 Sixel Data Encoding

Each sixel character represents 6 vertical pixels:
- Valid range: `?` (0x3F) to `~` (0x7E)
- Binary value = character code - 0x3F
- LSB = topmost pixel

Example:
```
? = 0x3F - 0x3F = 0 = 000000 (6 empty pixels)
~ = 0x7E - 0x3F = 63 = 111111 (6 filled pixels)
```

#### 2.2.4 Control Characters

| Char | Code | Function |
|------|------|----------|
| `!` | 0x21 | Repeat: `!<count><char>` |
| `"` | 0x22 | Raster attributes: `"<pan>;<pad>;<ph>;<pv>` |
| `#` | 0x23 | Color: `#<n>` or `#<n>;<type>;<p1>;<p2>;<p3>` |
| `$` | 0x24 | Graphics carriage return |
| `-` | 0x2D | Graphics newline |

#### 2.2.5 Color Specification

**Selection:** `#<color_number>` (0-255)

**Definition:** `#<color_number>;<type>;<p1>;<p2>;<p3>`

| Type | Color Space | Parameters |
|------|-------------|------------|
| 1 | HLS | Hue (0-360), Lightness (0-100), Saturation (0-100) |
| 2 | RGB | Red (0-100), Green (0-100), Blue (0-100) |

#### 2.2.6 Raster Attributes

Format: `"<pan>;<pad>;<ph>;<pv>`

| Parameter | Description |
|-----------|-------------|
| pan | Pixel aspect ratio numerator |
| pad | Pixel aspect ratio denominator |
| ph | Horizontal extent (pixels) |
| pv | Vertical extent (pixels) |

#### 2.2.7 Sixel Display Mode (DECSDM)

- Enable: `CSI ? 80 h`
- Disable: `CSI ? 80 l`

---

## 3. Image Rendering Pipeline

### 3.1 Architecture Overview

```
+----------------+     +------------------+     +----------------+
| ANSI Parser    | --> | Image Processor  | --> | Renderer       |
| (Rust)         |     | (Rust/Async)     |     | (TypeScript)   |
+----------------+     +------------------+     +----------------+
       |                       |                       |
       v                       v                       v
  Parse escape           Decode/store            Display in
  sequences              RGBA data               WebView
```

### 3.2 Parser State Machine

Extend existing ANSI parser to handle:

**For Kitty Graphics:**
- New state: `ApcString` (after `ESC _`)
- Accumulate until `ESC \` (ST)
- Parse control data and Base64 payload

**For SIXEL:**
- New state: `DcsString` (after `ESC P` or 0x90)
- Parse P1, P2, P3 parameters
- Accumulate sixel data until ST

### 3.3 Processing Pipeline

1. **Sequence Detection**: Parser identifies graphics sequence
2. **Buffering**: Accumulate chunks until sequence complete
3. **Validation**: Verify parameters and data integrity
4. **Decoding**: Convert to RGBA (async, off main thread)
5. **Storage**: Store with assigned/provided ID
6. **Placement**: Calculate display position
7. **Rendering**: Send to frontend for WebView display

### 3.4 Async Processing

```rust
// Conceptual structure
pub struct ImageProcessor {
    decoder_pool: ThreadPool,
    image_store: Arc<RwLock<ImageStore>>,
    pending_chunks: HashMap<u32, ChunkAccumulator>,
}

impl ImageProcessor {
    pub async fn process_kitty_command(&self, cmd: KittyCommand) -> Result<Response>;
    pub async fn process_sixel(&self, data: SixelData) -> Result<ImageId>;
}
```

---

## 4. Animation Support

### 4.1 Frame Management

```rust
pub struct AnimatedImage {
    id: u32,
    frames: Vec<Frame>,
    current_frame: usize,
    loop_count: Option<u32>,
    loops_completed: u32,
    state: AnimationState,
}

pub struct Frame {
    data: Vec<u8>,  // RGBA
    width: u32,
    height: u32,
    delay_ms: u32,
    dispose_mode: DisposeMode,
    blend_mode: BlendMode,
}

pub enum AnimationState {
    Stopped,
    Playing,
    Loading,  // Waiting for more frames
}
```

### 4.2 Animation Loop

```typescript
class AnimationController {
  private animations: Map<number, AnimatedImage>;
  private rafId: number | null = null;

  start(imageId: number): void;
  stop(imageId: number): void;
  setFrame(imageId: number, frame: number): void;

  private tick(timestamp: number): void {
    for (const [id, anim] of this.animations) {
      if (anim.state === 'playing' && this.isVisible(id)) {
        this.updateFrame(id, timestamp);
      }
    }
    this.rafId = requestAnimationFrame(this.tick.bind(this));
  }
}
```

### 4.3 Frame Composition

For Kitty `a=c` (compose) action:

```rust
pub fn compose_frames(
    src_frame: &Frame,
    dst_frame: &mut Frame,
    src_rect: Rect,
    dst_offset: Point,
    blend: BlendMode,
) -> Result<()>;
```

---

## 5. Memory Management Strategy

### 5.1 Storage Quota

```rust
pub struct ImageStore {
    images: HashMap<u32, StoredImage>,
    total_size: usize,
    max_size: usize,  // Default: 320MB
    lru_order: VecDeque<u32>,
}

impl ImageStore {
    pub fn insert(&mut self, id: u32, image: StoredImage) -> Result<()> {
        while self.total_size + image.size() > self.max_size {
            self.evict_oldest()?;
        }
        // Insert new image
    }

    fn evict_oldest(&mut self) -> Result<()>;
}
```

### 5.2 Image Lifecycle States

```rust
pub enum ImageState {
    Receiving { chunks: Vec<Chunk>, expected: Option<usize> },
    Decoding,
    Ready { data: Arc<[u8]>, placements: Vec<Placement> },
    Error(String),
}
```

### 5.3 Placement Management

```rust
pub struct Placement {
    id: u32,
    image_id: u32,
    position: CellPosition,
    source_rect: Option<Rect>,
    display_size: Option<(u16, u16)>,  // columns, rows
    z_index: i32,
    visible: bool,
}

pub struct PlacementManager {
    placements: HashMap<(u32, u32), Placement>,  // (image_id, placement_id)
    by_position: BTreeMap<CellPosition, Vec<(u32, u32)>>,
    by_z_index: BTreeMap<i32, Vec<(u32, u32)>>,
}
```

---

## 6. Resize Behavior

### 6.1 Resize Detection

```typescript
class ResizeHandler {
  private debounceTimer: number | null = null;
  private readonly DEBOUNCE_MS = 100;

  onResize(): void {
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
    }
    this.debounceTimer = setTimeout(() => {
      this.performResize();
    }, this.DEBOUNCE_MS);
  }

  private performResize(): void {
    const newCellSize = this.calculateCellSize();
    this.rescaleAllPlacements(newCellSize);
  }
}
```

### 6.2 Scaling Algorithm

```typescript
interface ScalingOptions {
  algorithm: 'bilinear' | 'nearest';
  maintainAspectRatio: boolean;
}

function scaleImage(
  source: ImageData,
  targetWidth: number,
  targetHeight: number,
  options: ScalingOptions
): ImageData;
```

### 6.3 Progressive Rendering

For large images during resize:

1. Immediately display low-resolution version
2. Asynchronously render full-resolution
3. Replace when complete

---

## 7. Performance Considerations

### 7.1 Latency Budget

| Operation | Budget |
|-----------|--------|
| Input processing | < 1ms |
| Small image decode (< 100KB) | < 50ms |
| Large image decode (< 5MB) | < 500ms |
| Frame render | < 16ms (60fps) |

### 7.2 Threading Model

```
Main Thread (Rust)
  |
  +-- ANSI Parser (sync, fast path for text)
  |
  +-- Image Command Router
        |
        +-- Decoder Thread Pool
        |     |
        |     +-- PNG Decoder
        |     +-- JPEG Decoder
        |     +-- SIXEL Decoder
        |     +-- GIF/Animation Decoder
        |
        +-- Storage Manager (mutex-protected)

Main Thread (TypeScript)
  |
  +-- Terminal Renderer
  |
  +-- Image Layer (Canvas/WebGL)
  |
  +-- Animation Controller (requestAnimationFrame)
```

### 7.3 Optimization Strategies

1. **Fast Path for Text**: Image sequences bypass normal text processing
2. **Lazy Decoding**: Decode only when image enters viewport
3. **Canvas Pooling**: Reuse canvas elements for image rendering
4. **Bitmap Caching**: Cache scaled versions at common sizes
5. **Visibility Culling**: Skip rendering for off-screen images

---

## 8. Error Handling

### 8.1 Error Categories

```rust
pub enum ImageError {
    // Protocol errors
    InvalidSequence(String),
    InvalidParameter { key: String, value: String },
    MissingRequiredParameter(String),

    // Image errors
    UnsupportedFormat(String),
    DecodeFailed(String),
    CorruptedData,

    // Resource errors
    QuotaExceeded { current: usize, max: usize },
    ImageNotFound(u32),
    PlacementNotFound(u32, u32),

    // System errors
    IoError(std::io::Error),
    MemoryAllocation,
}
```

### 8.2 Error Response Generation

```rust
impl ImageError {
    fn to_response(&self, image_id: u32) -> String {
        let code = match self {
            Self::InvalidParameter { .. } | Self::InvalidSequence(_) => "EINVAL",
            Self::ImageNotFound(_) | Self::PlacementNotFound(_, _) => "ENOENT",
            Self::QuotaExceeded { .. } => "ENOSPC",
            _ => "EFAILED",
        };
        format!("\x1b_Gi={};ERROR:{}\x1b\\", image_id, code)
    }
}
```

### 8.3 Recovery Strategies

| Error | Strategy |
|-------|----------|
| Decode failure | Display placeholder, log error |
| Quota exceeded | Evict LRU images, retry |
| Chunk timeout | Discard partial data, send error |
| Invalid sequence | Ignore, continue parsing |

---

## 9. Security Considerations

### 9.1 Input Validation

- Maximum image dimensions: 16384 x 16384 pixels
- Maximum chunk size: 4096 bytes
- Maximum total image size: 100MB per image
- Timeout for incomplete sequences: 30 seconds

### 9.2 File Path Handling

For `t=f` and `t=t` transmission:

```rust
fn validate_file_path(path: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path);

    // Must be absolute
    if !path.is_absolute() {
        return Err(ImageError::InvalidPath("relative path not allowed"));
    }

    // Canonicalize to resolve symlinks
    let canonical = path.canonicalize()?;

    // Check against allowed directories
    if !is_in_allowed_directory(&canonical) {
        return Err(ImageError::InvalidPath("path outside sandbox"));
    }

    Ok(canonical)
}
```

### 9.3 Shared Memory Handling

For `t=s` transmission:

```rust
fn open_shared_memory(name: &str, size: usize) -> Result<MmapMut> {
    // Validate name (no path traversal)
    if name.contains('/') || name.contains('\\') {
        return Err(ImageError::InvalidPath("invalid shm name"));
    }

    // Limit size
    if size > MAX_IMAGE_SIZE {
        return Err(ImageError::QuotaExceeded { current: size, max: MAX_IMAGE_SIZE });
    }

    // Open with appropriate permissions
    // ...
}
```

### 9.4 DoS Prevention

- Rate limiting for image commands
- Maximum concurrent decoding operations
- Timeout for animation frames
- Memory pressure monitoring

---

## 10. Frontend Integration

### 10.1 Image Layer Architecture

```typescript
interface ImageLayer {
  // Image management
  addImage(id: number, data: ImageData, position: CellPosition): void;
  removeImage(id: number): void;
  updatePosition(id: number, position: CellPosition): void;

  // Rendering
  render(visibleRange: CellRange): void;

  // Events
  onScroll(callback: (range: CellRange) => void): void;
  onResize(callback: (cellSize: Size) => void): void;
}
```

### 10.2 Canvas-based Rendering

```typescript
class CanvasImageLayer implements ImageLayer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private images: Map<number, ImagePlacement>;

  render(visibleRange: CellRange): void {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);

    for (const [id, placement] of this.images) {
      if (this.intersects(placement, visibleRange)) {
        this.drawPlacement(placement);
      }
    }
  }
}
```

### 10.3 Communication with Backend

```typescript
// IPC message types
interface ImageMessage {
  type: 'add' | 'remove' | 'update' | 'animate';
  imageId: number;
  data?: ArrayBuffer;  // RGBA data
  position?: CellPosition;
  size?: { width: number; height: number };
  zIndex?: number;
}

// Tauri command integration
async function handleImageCommand(msg: ImageMessage): Promise<void>;
```

---

## 11. Testing Strategy

### 11.1 Unit Tests

- Parser state machine transitions
- SIXEL encoding/decoding
- Kitty command parsing
- Memory quota management
- Image scaling algorithms

### 11.2 Integration Tests

- End-to-end image display
- Animation playback
- Resize behavior
- Error recovery

### 11.3 Compatibility Tests

- Kitty icat tool compatibility
- libsixel output compatibility
- Various image formats (PNG, JPEG, GIF)

### 11.4 Performance Tests

- Decode latency benchmarks
- Memory usage under load
- Animation frame rate
- Input latency during image processing

---

## 12. Implementation Phases

### Phase 1: Core Infrastructure
- Extend ANSI parser for APC/DCS sequences
- Implement basic Kitty `a=t`, `a=T`, `a=p` actions
- Basic SIXEL parsing and rendering
- Frontend image layer (canvas-based)

### Phase 2: Full Protocol Support
- All Kitty actions (`a=d`, `a=q`)
- Chunked transfer and compression
- Full SIXEL color support
- Memory quota management

### Phase 3: Animation Support
- Kitty animation frames (`a=f`, `a=a`, `a=c`)
- GIF animation playback
- Animation control UI

### Phase 4: Optimization
- GPU-accelerated rendering (WebGL)
- Progressive image loading
- Advanced caching strategies
- Performance tuning
