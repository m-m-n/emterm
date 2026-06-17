/**
 * Shared utility functions for image processing.
 *
 * @module image/utils
 */

/**
 * Decodes a base64-encoded binary string into a Uint8ClampedArray.
 *
 * Commonly used to convert base64 RGBA image data into a byte array
 * suitable for ImageData construction.
 *
 * @param base64 - Base64-encoded string
 * @returns Decoded bytes as Uint8ClampedArray
 */
export function decodeBase64ToBytes(base64: string): Uint8ClampedArray<ArrayBuffer> {
	const binaryString = atob(base64);
	const bytes = new Uint8ClampedArray(binaryString.length);
	for (let i = 0; i < binaryString.length; i++) {
		bytes[i] = binaryString.charCodeAt(i);
	}
	return bytes;
}
