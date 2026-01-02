//! CSI parameter parsing utilities.
//!
//! This module provides functionality for parsing CSI sequence parameters.
//! CSI parameters are semicolon-separated numeric values that precede the final byte.

/// Maximum number of parameters allowed in a CSI sequence.
/// Most terminals limit this to prevent DoS attacks.
pub const MAX_PARAMS: usize = 32;

/// Maximum value for a single parameter.
/// Values above this are clamped.
pub const MAX_PARAM_VALUE: u16 = 9999;

/// Parser for CSI sequence parameters.
///
/// Handles parsing of numeric parameters separated by semicolons.
/// Example: "1;31" parses to [1, 31]
#[derive(Debug, Clone)]
pub struct ParamParser {
    /// Collected parameters.
    params: Vec<u16>,
    /// Current parameter being built.
    current: u16,
    /// Whether we've started building a parameter.
    has_current: bool,
    /// Intermediate bytes (characters between ESC[ and parameters).
    intermediates: Vec<u8>,
}

impl Default for ParamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ParamParser {
    /// Creates a new parameter parser.
    pub fn new() -> Self {
        Self {
            params: Vec::with_capacity(8),
            current: 0,
            has_current: false,
            intermediates: Vec::new(),
        }
    }

    /// Resets the parser state.
    pub fn reset(&mut self) {
        self.params.clear();
        self.current = 0;
        self.has_current = false;
        self.intermediates.clear();
    }

    /// Adds an intermediate byte (e.g., '?' for DEC private modes).
    pub fn add_intermediate(&mut self, byte: u8) {
        if self.intermediates.len() < 4 {
            self.intermediates.push(byte);
        }
    }

    /// Returns the intermediate bytes.
    pub fn intermediates(&self) -> &[u8] {
        &self.intermediates
    }

    /// Checks if this is a DEC private mode sequence (has '?' intermediate).
    pub fn is_dec_private(&self) -> bool {
        self.intermediates.first() == Some(&b'?')
    }

    /// Adds a digit to the current parameter.
    ///
    /// # Arguments
    ///
    /// * `digit` - ASCII digit character ('0'-'9')
    pub fn add_digit(&mut self, digit: u8) {
        let digit_value = (digit - b'0') as u16;
        self.current = self.current.saturating_mul(10).saturating_add(digit_value);
        if self.current > MAX_PARAM_VALUE {
            self.current = MAX_PARAM_VALUE;
        }
        self.has_current = true;
    }

    /// Completes the current parameter and starts a new one.
    /// Called when a semicolon is encountered.
    pub fn finish_param(&mut self) {
        if self.params.len() < MAX_PARAMS {
            // Use 0 as default if no digits were provided (e.g., ";;")
            self.params
                .push(if self.has_current { self.current } else { 0 });
        }
        self.current = 0;
        self.has_current = false;
    }

    /// Finalizes parsing and returns the collected parameters.
    ///
    /// This should be called when the final byte of a CSI sequence is reached.
    pub fn finish(&mut self) -> Vec<u16> {
        // Add the last parameter if we have one or if we've seen any separator
        if (self.has_current || !self.params.is_empty()) && self.params.len() < MAX_PARAMS {
            self.params
                .push(if self.has_current { self.current } else { 0 });
        }

        std::mem::take(&mut self.params)
    }

    /// Gets a parameter value with a default.
    ///
    /// # Arguments
    ///
    /// * `params` - The parameter slice
    /// * `index` - Parameter index
    /// * `default` - Default value if parameter is missing or zero
    pub fn get_param(params: &[u16], index: usize, default: u16) -> u16 {
        params
            .get(index)
            .copied()
            .filter(|&v| v != 0)
            .unwrap_or(default)
    }

    /// Gets the first parameter with a default of 1.
    /// Many CSI commands default to 1 for the first parameter.
    pub fn get_first_or_one(params: &[u16]) -> u16 {
        Self::get_param(params, 0, 1)
    }

    /// Gets the first parameter with a default of 0.
    pub fn get_first_or_zero(params: &[u16]) -> u16 {
        params.first().copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_params() {
        let mut parser = ParamParser::new();
        let params = parser.finish();
        assert!(params.is_empty());
    }

    #[test]
    fn test_single_param() {
        let mut parser = ParamParser::new();
        parser.add_digit(b'3');
        parser.add_digit(b'1');
        let params = parser.finish();
        assert_eq!(params, vec![31]);
    }

    #[test]
    fn test_multiple_params() {
        let mut parser = ParamParser::new();
        // Parse "1;31"
        parser.add_digit(b'1');
        parser.finish_param();
        parser.add_digit(b'3');
        parser.add_digit(b'1');
        let params = parser.finish();
        assert_eq!(params, vec![1, 31]);
    }

    #[test]
    fn test_three_params() {
        let mut parser = ParamParser::new();
        // Parse "38;5;196"
        parser.add_digit(b'3');
        parser.add_digit(b'8');
        parser.finish_param();
        parser.add_digit(b'5');
        parser.finish_param();
        parser.add_digit(b'1');
        parser.add_digit(b'9');
        parser.add_digit(b'6');
        let params = parser.finish();
        assert_eq!(params, vec![38, 5, 196]);
    }

    #[test]
    fn test_empty_param_defaults_to_zero() {
        let mut parser = ParamParser::new();
        // Parse ";31" (missing first param)
        parser.finish_param(); // Empty first param
        parser.add_digit(b'3');
        parser.add_digit(b'1');
        let params = parser.finish();
        assert_eq!(params, vec![0, 31]);
    }

    #[test]
    fn test_trailing_semicolon() {
        let mut parser = ParamParser::new();
        // Parse "1;"
        parser.add_digit(b'1');
        parser.finish_param();
        let params = parser.finish();
        assert_eq!(params, vec![1, 0]);
    }

    #[test]
    fn test_param_overflow_clamped() {
        let mut parser = ParamParser::new();
        // Parse a very large number
        for _ in 0..10 {
            parser.add_digit(b'9');
        }
        let params = parser.finish();
        assert_eq!(params, vec![MAX_PARAM_VALUE]);
    }

    #[test]
    fn test_intermediate_bytes() {
        let mut parser = ParamParser::new();
        parser.add_intermediate(b'?');
        assert!(parser.is_dec_private());
        assert_eq!(parser.intermediates(), &[b'?']);
    }

    #[test]
    fn test_get_param_with_default() {
        let params = vec![1, 31, 0];
        assert_eq!(ParamParser::get_param(&params, 0, 5), 1);
        assert_eq!(ParamParser::get_param(&params, 1, 5), 31);
        assert_eq!(ParamParser::get_param(&params, 2, 5), 5); // 0 treated as missing
        assert_eq!(ParamParser::get_param(&params, 3, 5), 5); // Out of bounds
    }

    #[test]
    fn test_get_first_or_one() {
        assert_eq!(ParamParser::get_first_or_one(&[]), 1);
        assert_eq!(ParamParser::get_first_or_one(&[0]), 1);
        assert_eq!(ParamParser::get_first_or_one(&[5]), 5);
    }

    #[test]
    fn test_get_first_or_zero() {
        assert_eq!(ParamParser::get_first_or_zero(&[]), 0);
        assert_eq!(ParamParser::get_first_or_zero(&[0]), 0);
        assert_eq!(ParamParser::get_first_or_zero(&[5]), 5);
    }

    #[test]
    fn test_reset() {
        let mut parser = ParamParser::new();
        parser.add_digit(b'1');
        parser.add_intermediate(b'?');
        parser.reset();
        let params = parser.finish();
        assert!(params.is_empty());
        assert!(parser.intermediates().is_empty());
    }

    #[test]
    fn test_max_params_limit() {
        let mut parser = ParamParser::new();
        // Try to add more than MAX_PARAMS parameters
        for _ in 0..MAX_PARAMS + 10 {
            parser.add_digit(b'1');
            parser.finish_param();
        }
        let params = parser.finish();
        // Should be limited to MAX_PARAMS + 1 (for the final finish())
        // Actually, we check in finish_param, so it should be MAX_PARAMS
        assert!(params.len() <= MAX_PARAMS + 1);
    }
}
