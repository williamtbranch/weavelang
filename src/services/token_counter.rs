use tiktoken_rs::cl100k_base;

pub struct TokenCounter;

impl TokenCounter {
    /// Estimates token count using cl100k_base encoding (GPT-4 standard).
    /// This is a good proxy for most modern LLMs including Anthropic for estimation purposes.
    pub fn count_tokens(text: &str) -> usize {
        // cl100k_base() creates a new BPE instance. 
        // In a real app, we might want to cache this instance or use a singleton if performance is critical.
        // For batch processing, it's fine.
        match cl100k_base() {
            Ok(bpe) => bpe.encode_with_special_tokens(text).len(),
            Err(_) => {
                // Fallback: Estimate 4 chars per token
                text.len() / 4
            }
        }
    }

    pub fn estimate_cost(model: &str, input_tokens: usize, output_tokens: usize) -> f64 {
        // Pricing as of Feb 2026 (hypothetical or based on latest known)
        // Claude 3 Haiku: $0.25 / 1M input, $1.25 / 1M output
        // Claude 3.5 Sonnet: $3.00 / 1M input, $15.00 / 1M output
        
        let (input_rate, output_rate) = match model {
            m if m.contains("haiku") => (0.25, 1.25),
            m if m.contains("sonnet") => (3.00, 15.00),
            m if m.contains("opus") => (15.00, 75.00),
            _ => (3.00, 15.00), // Default to Sonnet pricing
        };

        (input_tokens as f64 / 1_000_000.0) * input_rate + (output_tokens as f64 / 1_000_000.0) * output_rate
    }
}
