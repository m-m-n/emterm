/**
 * WebGL-accelerated image rendering layer.
 *
 * Provides hardware-accelerated image rendering for terminal.
 * Falls back to Canvas 2D when WebGL is not available.
 *
 * @module image/webgl-layer
 */

/**
 * Check if WebGL is supported in the current environment.
 *
 * @returns True if WebGL (1 or 2) is supported
 */
export function isWebGLSupported(): boolean {
	try {
		const canvas = document.createElement("canvas");
		return !!(canvas.getContext("webgl2") || canvas.getContext("webgl"));
	} catch {
		return false;
	}
}

/**
 * WebGL texture information.
 */
interface TextureInfo {
	/** WebGL texture object. */
	texture: WebGLTexture;

	/** Texture width. */
	width: number;

	/** Texture height. */
	height: number;
}

/**
 * Placement information for rendering.
 */
export interface WebGLPlacement {
	/** Texture ID to render. */
	textureId: number;

	/** X position in pixels. */
	x: number;

	/** Y position in pixels. */
	y: number;

	/** Display width in pixels. */
	width: number;

	/** Display height in pixels. */
	height: number;

	/** Z-index for layering. */
	zIndex: number;

	/** Optional unique key for this placement. */
	key?: string;
}

/** Vertex shader source. */
const VERTEX_SHADER_SOURCE = `
  attribute vec2 a_position;
  attribute vec2 a_texCoord;

  uniform vec2 u_resolution;

  varying vec2 v_texCoord;

  void main() {
    // Convert from pixel coordinates to clip space (-1 to 1)
    vec2 clipSpace = (a_position / u_resolution) * 2.0 - 1.0;

    // Flip Y axis (WebGL Y is up, screen Y is down)
    gl_Position = vec4(clipSpace.x, -clipSpace.y, 0.0, 1.0);

    v_texCoord = a_texCoord;
  }
`;

/** Fragment shader source. */
const FRAGMENT_SHADER_SOURCE = `
  precision mediump float;

  uniform sampler2D u_texture;

  varying vec2 v_texCoord;

  void main() {
    gl_FragColor = texture2D(u_texture, v_texCoord);
  }
`;

/**
 * WebGL-accelerated image layer.
 *
 * Uses WebGL for hardware-accelerated texture rendering.
 * Supports multiple textures and z-index sorted rendering.
 */
export class WebGLLayer {
	/** Canvas element. */
	private canvas: HTMLCanvasElement;

	/** WebGL rendering context. */
	private gl: WebGLRenderingContext | WebGL2RenderingContext | null = null;

	/** Whether WebGL is active. */
	private webglActive: boolean = false;

	/** Shader program. */
	private program: WebGLProgram | null = null;

	/** Textures by ID. */
	private textures: Map<number, TextureInfo> = new Map();

	/** Active placements. */
	private placements: Map<string, WebGLPlacement> = new Map();

	/** Vertex buffer. */
	private vertexBuffer: WebGLBuffer | null = null;

	/** Texture coordinate buffer. */
	private texCoordBuffer: WebGLBuffer | null = null;

	/** Attribute locations. */
	private positionLocation: number = -1;
	private texCoordLocation: number = -1;

	/** Uniform locations. */
	private resolutionLocation: WebGLUniformLocation | null = null;
	private textureLocation: WebGLUniformLocation | null = null;

	/** Canvas dimensions. */
	private width: number = 0;
	private height: number = 0;

	/** Auto-generated placement key counter. */
	private placementCounter: number = 0;

	/**
	 * Create a new WebGL layer.
	 *
	 * @param container - Parent element to attach canvas to
	 */
	constructor(container: HTMLElement) {
		this.canvas = document.createElement("canvas");
		this.canvas.className = "terminal-webgl-layer";
		this.canvas.style.cssText = `
      position: absolute;
      top: 0;
      left: 0;
      pointer-events: none;
      z-index: -1;
    `;

		// Try to get WebGL context
		this.gl = this.canvas.getContext("webgl2") as WebGL2RenderingContext | null;
		if (!this.gl) {
			this.gl = this.canvas.getContext("webgl") as WebGLRenderingContext | null;
		}

		if (this.gl) {
			this.webglActive = true;
			this.initWebGL();
		}

		// Insert canvas
		if (container.firstChild) {
			container.insertBefore(this.canvas, container.firstChild);
		} else {
			container.appendChild(this.canvas);
		}

		// Ensure container has relative positioning
		const containerStyle = getComputedStyle(container);
		if (containerStyle.position === "static") {
			container.style.position = "relative";
		}
	}

	/**
	 * Initialize WebGL resources.
	 */
	private initWebGL(): void {
		const gl = this.gl!;

		// Create shader program
		const vertexShader = this.createShader(
			gl.VERTEX_SHADER,
			VERTEX_SHADER_SOURCE,
		);
		const fragmentShader = this.createShader(
			gl.FRAGMENT_SHADER,
			FRAGMENT_SHADER_SOURCE,
		);

		if (!vertexShader || !fragmentShader) {
			this.webglActive = false;
			return;
		}

		this.program = gl.createProgram();
		if (!this.program) {
			this.webglActive = false;
			return;
		}

		gl.attachShader(this.program, vertexShader);
		gl.attachShader(this.program, fragmentShader);
		gl.linkProgram(this.program);

		if (!gl.getProgramParameter(this.program, gl.LINK_STATUS)) {
			console.warn("WebGL program link failed");
			this.webglActive = false;
			return;
		}

		// Get attribute and uniform locations
		this.positionLocation = gl.getAttribLocation(this.program, "a_position");
		this.texCoordLocation = gl.getAttribLocation(this.program, "a_texCoord");
		this.resolutionLocation = gl.getUniformLocation(
			this.program,
			"u_resolution",
		);
		this.textureLocation = gl.getUniformLocation(this.program, "u_texture");

		// Create buffers
		this.vertexBuffer = gl.createBuffer();
		this.texCoordBuffer = gl.createBuffer();

		// Set up texture coordinates (same for all quads)
		gl.bindBuffer(gl.ARRAY_BUFFER, this.texCoordBuffer);
		gl.bufferData(
			gl.ARRAY_BUFFER,
			new Float32Array([
				0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0,
			]),
			gl.STATIC_DRAW,
		);

		// Enable alpha blending
		gl.enable(gl.BLEND);
		gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
	}

	/**
	 * Create a WebGL shader.
	 */
	private createShader(type: number, source: string): WebGLShader | null {
		const gl = this.gl!;
		const shader = gl.createShader(type);
		if (!shader) return null;

		gl.shaderSource(shader, source);
		gl.compileShader(shader);

		if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
			console.warn("WebGL shader compile failed:", gl.getShaderInfoLog(shader));
			gl.deleteShader(shader);
			return null;
		}

		return shader;
	}

	/**
	 * Check if WebGL is active.
	 *
	 * @returns True if WebGL is being used
	 */
	isWebGLActive(): boolean {
		return this.webglActive;
	}

	/**
	 * Set canvas size.
	 *
	 * @param width - Width in pixels
	 * @param height - Height in pixels
	 */
	setCanvasSize(width: number, height: number): void {
		const dpr = window.devicePixelRatio || 1;

		this.width = width;
		this.height = height;

		this.canvas.width = width * dpr;
		this.canvas.height = height * dpr;
		this.canvas.style.width = `${width}px`;
		this.canvas.style.height = `${height}px`;

		if (this.gl) {
			this.gl.viewport(0, 0, this.canvas.width, this.canvas.height);
		}
	}

	/**
	 * Upload RGBA data as a texture.
	 *
	 * @param id - Texture ID
	 * @param rgba - RGBA pixel data
	 * @param width - Image width
	 * @param height - Image height
	 * @returns Texture ID
	 */
	uploadTexture(
		id: number,
		rgba: Uint8ClampedArray,
		width: number,
		height: number,
	): number {
		if (!this.gl || !this.webglActive) return id;

		const gl = this.gl;

		// Delete existing texture if present
		const existing = this.textures.get(id);
		if (existing) {
			gl.deleteTexture(existing.texture);
		}

		// Create new texture
		const texture = gl.createTexture();
		if (!texture) return id;

		gl.bindTexture(gl.TEXTURE_2D, texture);

		// Set texture parameters
		gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
		gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
		gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
		gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);

		// Upload texture data
		gl.texImage2D(
			gl.TEXTURE_2D,
			0,
			gl.RGBA,
			width,
			height,
			0,
			gl.RGBA,
			gl.UNSIGNED_BYTE,
			rgba,
		);

		this.textures.set(id, { texture, width, height });
		return id;
	}

	/**
	 * Delete a texture.
	 *
	 * @param id - Texture ID
	 */
	deleteTexture(id: number): void {
		const info = this.textures.get(id);
		if (info && this.gl) {
			this.gl.deleteTexture(info.texture);
			this.textures.delete(id);
		}
	}

	/**
	 * Check if a texture exists.
	 *
	 * @param id - Texture ID
	 * @returns True if texture exists
	 */
	hasTexture(id: number): boolean {
		return this.textures.has(id);
	}

	/**
	 * Get texture count.
	 *
	 * @returns Number of textures
	 */
	getTextureCount(): number {
		return this.textures.size;
	}

	/**
	 * Add a placement.
	 *
	 * @param placement - Placement specification
	 */
	addPlacement(placement: WebGLPlacement): void {
		const key = placement.key ?? `auto-${this.placementCounter++}`;
		this.placements.set(key, { ...placement, key });
	}

	/**
	 * Remove a placement by key.
	 *
	 * @param key - Placement key
	 */
	removePlacement(key: string): void {
		this.placements.delete(key);
	}

	/**
	 * Clear all placements.
	 */
	clearPlacements(): void {
		this.placements.clear();
	}

	/**
	 * Get placement count.
	 *
	 * @returns Number of placements
	 */
	getPlacementCount(): number {
		return this.placements.size;
	}

	/**
	 * Render all placements.
	 */
	render(): void {
		if (!this.gl || !this.webglActive || !this.program) return;

		const gl = this.gl;
		const dpr = window.devicePixelRatio || 1;

		// Clear canvas
		gl.clearColor(0, 0, 0, 0);
		gl.clear(gl.COLOR_BUFFER_BIT);

		// Use shader program
		gl.useProgram(this.program);

		// Set resolution uniform
		gl.uniform2f(this.resolutionLocation, this.width * dpr, this.height * dpr);

		// Sort placements by z-index
		const sorted = Array.from(this.placements.values()).sort(
			(a, b) => a.zIndex - b.zIndex,
		);

		// Enable vertex attributes
		gl.enableVertexAttribArray(this.positionLocation);
		gl.enableVertexAttribArray(this.texCoordLocation);

		// Bind texture coordinates
		gl.bindBuffer(gl.ARRAY_BUFFER, this.texCoordBuffer);
		gl.vertexAttribPointer(this.texCoordLocation, 2, gl.FLOAT, false, 0, 0);

		// Draw each placement
		for (const placement of sorted) {
			const textureInfo = this.textures.get(placement.textureId);
			if (!textureInfo) continue;

			// Bind texture
			gl.activeTexture(gl.TEXTURE0);
			gl.bindTexture(gl.TEXTURE_2D, textureInfo.texture);
			gl.uniform1i(this.textureLocation, 0);

			// Set up vertex positions for this quad
			const x1 = placement.x * dpr;
			const y1 = placement.y * dpr;
			const x2 = (placement.x + placement.width) * dpr;
			const y2 = (placement.y + placement.height) * dpr;

			gl.bindBuffer(gl.ARRAY_BUFFER, this.vertexBuffer);
			gl.bufferData(
				gl.ARRAY_BUFFER,
				new Float32Array([x1, y1, x2, y1, x1, y2, x1, y2, x2, y1, x2, y2]),
				gl.STATIC_DRAW,
			);
			gl.vertexAttribPointer(this.positionLocation, 2, gl.FLOAT, false, 0, 0);

			// Draw quad
			gl.drawArrays(gl.TRIANGLES, 0, 6);
		}
	}

	/**
	 * Get the canvas element.
	 *
	 * @returns Canvas element
	 */
	getCanvas(): HTMLCanvasElement {
		return this.canvas;
	}

	/**
	 * Dispose of the WebGL layer.
	 */
	dispose(): void {
		if (this.gl) {
			// Delete textures
			for (const info of this.textures.values()) {
				this.gl.deleteTexture(info.texture);
			}
			this.textures.clear();

			// Delete buffers
			if (this.vertexBuffer) {
				this.gl.deleteBuffer(this.vertexBuffer);
			}
			if (this.texCoordBuffer) {
				this.gl.deleteBuffer(this.texCoordBuffer);
			}

			// Delete program
			if (this.program) {
				this.gl.deleteProgram(this.program);
			}
		}

		this.placements.clear();
		this.canvas.remove();
		this.webglActive = false;
	}
}
