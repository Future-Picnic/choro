use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub reported_total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
}

impl UsageTotals {
    pub fn total_tokens(&self) -> u64 {
        if self.reported_total_tokens > 0 {
            return self.reported_total_tokens;
        }
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.reasoning_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }

    pub(crate) fn add_assign(&mut self, other: &Self) {
        self.reported_total_tokens = self
            .reported_total_tokens
            .saturating_add(other.total_tokens());
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.cost_usd += other.cost_usd;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub provider_id: String,
    pub model_id: String,
    pub totals: UsageTotals,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConversationUsage {
    /// The provider session represented by this snapshot. Re-fetching the
    /// complete provider session makes resume additive without double-counting.
    pub session_id: String,
    pub totals: UsageTotals,
    pub latest_turn: Option<UsageTotals>,
    pub models: Vec<ModelUsage>,
}
