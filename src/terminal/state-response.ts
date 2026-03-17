/**
 * Terminal state response management.
 *
 * Handles pending device response bytes that need to be written back to PTY.
 * Extracted from TerminalState for separation of concerns.
 */

/**
 * Get and clear all pending response bytes.
 * Returns all buffered responses concatenated together to handle multiple DSRs.
 *
 * @param pendingResponses - The pending responses array (will be emptied)
 * @returns Combined response bytes, or null if no responses pending
 */
export function takePendingResponse(
  pendingResponses: Uint8Array[],
): { result: Uint8Array | null; remaining: Uint8Array[] } {
  if (pendingResponses.length === 0) {
    return { result: null, remaining: [] };
  }

  // Concatenate all pending responses
  const totalLength = pendingResponses.reduce(
    (sum, r) => sum + r.length,
    0,
  );
  const combined = new Uint8Array(totalLength);
  let offset = 0;
  for (const response of pendingResponses) {
    combined.set(response, offset);
    offset += response.length;
  }

  return { result: combined, remaining: [] };
}

/**
 * Add a response to the pending response buffer.
 *
 * @param pendingResponses - The pending responses array
 * @param response - Response bytes to add
 * @returns Updated pending responses array
 */
export function addPendingResponse(
  pendingResponses: Uint8Array[],
  response: Uint8Array,
): Uint8Array[] {
  pendingResponses.push(response);
  return pendingResponses;
}
