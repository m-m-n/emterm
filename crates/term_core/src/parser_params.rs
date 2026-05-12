/// Maximum number of parameters allowed in a CSI sequence.
pub const MAX_PARAMS: usize = 32;

/// Maximum value for a single parameter.
pub const MAX_PARAM_VALUE: u16 = 9999;

/// Parser for CSI sequence parameters.
#[derive(Debug, Clone)]
pub(crate) struct ParamParser {
    params: Vec<u16>,
    current: u16,
    has_current: bool,
    intermediates: Vec<u8>,
}

impl Default for ParamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ParamParser {
    pub fn new() -> Self {
        Self {
            params: Vec::with_capacity(8),
            current: 0,
            has_current: false,
            intermediates: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.params.clear();
        self.current = 0;
        self.has_current = false;
        self.intermediates.clear();
    }

    pub fn add_intermediate(&mut self, byte: u8) {
        if self.intermediates.len() < 4 {
            self.intermediates.push(byte);
        }
    }

    pub fn intermediates(&self) -> &[u8] {
        &self.intermediates
    }

    #[cfg(test)]
    pub fn is_dec_private(&self) -> bool {
        self.intermediates.first() == Some(&b'?')
    }

    pub fn add_digit(&mut self, digit: u8) {
        let digit_value = (digit - b'0') as u16;
        self.current = self.current.saturating_mul(10).saturating_add(digit_value);
        if self.current > MAX_PARAM_VALUE {
            self.current = MAX_PARAM_VALUE;
        }
        self.has_current = true;
    }

    pub fn finish_param(&mut self) {
        if self.params.len() < MAX_PARAMS {
            self.params
                .push(if self.has_current { self.current } else { 0 });
        }
        self.current = 0;
        self.has_current = false;
    }

    pub fn finish(&mut self) -> Vec<u16> {
        if (self.has_current || !self.params.is_empty()) && self.params.len() < MAX_PARAMS {
            self.params
                .push(if self.has_current { self.current } else { 0 });
        }

        std::mem::take(&mut self.params)
    }

    pub fn get_param(params: &[u16], index: usize, default: u16) -> u16 {
        params
            .get(index)
            .copied()
            .filter(|&v| v != 0)
            .unwrap_or(default)
    }

    pub fn get_first_or_one(params: &[u16]) -> u16 {
        Self::get_param(params, 0, 1)
    }

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
        parser.finish_param();
        parser.add_digit(b'3');
        parser.add_digit(b'1');
        let params = parser.finish();
        assert_eq!(params, vec![0, 31]);
    }

    #[test]
    fn test_trailing_semicolon() {
        let mut parser = ParamParser::new();
        parser.add_digit(b'1');
        parser.finish_param();
        let params = parser.finish();
        assert_eq!(params, vec![1, 0]);
    }

    #[test]
    fn test_param_overflow_clamped() {
        let mut parser = ParamParser::new();
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
        assert_eq!(ParamParser::get_param(&params, 2, 5), 5);
        assert_eq!(ParamParser::get_param(&params, 3, 5), 5);
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
        for _ in 0..MAX_PARAMS + 10 {
            parser.add_digit(b'1');
            parser.finish_param();
        }
        let params = parser.finish();
        assert!(params.len() <= MAX_PARAMS + 1);
    }
}
