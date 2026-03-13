use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
    pub current_step: usize,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: usize,
    pub description: String,
    pub status: StepStatus,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Revised,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

impl Plan {
    pub fn new(goal: String) -> Self {
        Self {
            goal,
            steps: Vec::new(),
            current_step: 0,
            status: PlanStatus::Pending,
        }
    }

    pub fn add_step(&mut self, description: String) {
        self.steps.push(PlanStep {
            id: self.steps.len(),
            description,
            status: StepStatus::Pending,
            result: None,
        });
    }

    pub fn current_step(&self) -> Option<&PlanStep> {
        self.steps.get(self.current_step)
    }

    pub fn complete_step(&mut self, result: String) {
        if let Some(step) = self.steps.get_mut(self.current_step) {
            step.status = StepStatus::Completed;
            step.result = Some(result);
            self.current_step += 1;
        }
    }

    pub fn fail_step(&mut self, error: String) {
        if let Some(step) = self.steps.get_mut(self.current_step) {
            step.status = StepStatus::Failed;
            step.result = Some(error);
            self.status = PlanStatus::Failed;
        }
    }

    pub fn is_complete(&self) -> bool {
        self.current_step >= self.steps.len() && !self.steps.is_empty()
    }

    pub fn progress(&self) -> f32 {
        if self.steps.is_empty() {
            return 0.0;
        }
        self.current_step as f32 / self.steps.len() as f32
    }
}
