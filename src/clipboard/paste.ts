/**
 * Paste utilities for sending text to PTY with chunking.
 */

/**
 * Send text to PTY in chunks to prevent buffer overflow.
 *
 * Large pastes are sent in 1000-byte chunks with 50ms delays
 * to allow the PTY and shell to process the input.
 *
 * @param text - Text to send
 * @param writeFn - Function to write data to PTY
 * @returns Promise that resolves when all text is sent
 *
 * @example
 * ```ts
 * await sendTextInChunks(
 *   "Large text content",
 *   (data) => ptyClient.write(data)
 * );
 * ```
 */
export async function sendTextInChunks(
	text: string,
	writeFn: (data: Uint8Array) => Promise<void>,
): Promise<void> {
	const encoder = new TextEncoder();
	const bytes = encoder.encode(text);

	const CHUNK_SIZE = 1000; // 1000 bytes per chunk
	const CHUNK_DELAY = 50; // 50ms delay between chunks

	// Send in chunks if text is large
	if (bytes.length <= CHUNK_SIZE) {
		// Small text - send all at once
		await writeFn(bytes);
		return;
	}

	// Large text - send in chunks
	for (let offset = 0; offset < bytes.length; offset += CHUNK_SIZE) {
		const end = Math.min(offset + CHUNK_SIZE, bytes.length);
		const chunk = bytes.slice(offset, end);

		await writeFn(chunk);

		// Wait before sending next chunk (except for last chunk)
		if (end < bytes.length) {
			await new Promise((resolve) => setTimeout(resolve, CHUNK_DELAY));
		}
	}
}
