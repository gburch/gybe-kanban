use crate::activity_feed::ActivityEntityType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyLevel {
    Low,
    Normal,
    Elevated,
    High,
    Critical,
}

impl UrgencyLevel {
    fn base_score(self) -> u8 {
        match self {
            UrgencyLevel::Low => 10,
            UrgencyLevel::Normal => 35,
            UrgencyLevel::Elevated => 55,
            UrgencyLevel::High => 75,
            UrgencyLevel::Critical => 95,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UrgencyComputationContext {
    pub level: UrgencyLevel,
    pub recency_hours: u32,
    pub entity_type: ActivityEntityType,
}

pub fn calculate_score(context: UrgencyComputationContext) -> u8 {
    let mut score = context.level.base_score();

    // Recency penalty: every 6 hours reduces urgency slightly, up to 30 points
    let penalty_steps = (context.recency_hours / 6) as i32;
    let penalty = (penalty_steps * 2).min(20) as u8;
    score = score.saturating_sub(penalty);

    // Deployment and attempt events tend to demand faster attention
    match context.entity_type {
        ActivityEntityType::Deployment if score < 100 => {
            score = (score + 5).min(100);
        }
        ActivityEntityType::Attempt if score < 100 => {
            score = (score + 3).min(100);
        }
        _ => {}
    }

    score.clamp(0, 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_respect_level_and_recency() {
        let recent = UrgencyComputationContext {
            level: UrgencyLevel::High,
            recency_hours: 1,
            entity_type: ActivityEntityType::Task,
        };
        let old = UrgencyComputationContext {
            level: UrgencyLevel::High,
            recency_hours: 48,
            entity_type: ActivityEntityType::Task,
        };

        assert!(calculate_score(recent) > calculate_score(old));
    }

    #[test]
    fn deployment_events_get_small_bonus() {
        let ctx = UrgencyComputationContext {
            level: UrgencyLevel::Elevated,
            recency_hours: 0,
            entity_type: ActivityEntityType::Deployment,
        };
        assert!(calculate_score(ctx) >= 60);
    }
}
