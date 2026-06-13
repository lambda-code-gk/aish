//! capability policy port（application boundary 用）。

use crate::domain::Capability;

/// capability 不足時の拒否理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenied {
    pub capability: Capability,
    pub profile: String,
}

impl CapabilityDenied {
    pub fn message(&self) -> String {
        format!(
            "capability denied: {} (profile={})",
            self.capability.as_str(),
            self.profile
        )
    }
}

/// クライアント profile が持つ capability 集合。
pub trait CapabilityPolicy: Send + Sync {
    fn profile_name(&self) -> &str;

    fn allows(&self, capability: Capability) -> bool;

    fn require(&self, capability: Capability) -> Result<(), CapabilityDenied> {
        if self.allows(capability) {
            Ok(())
        } else {
            Err(CapabilityDenied {
                capability,
                profile: self.profile_name().to_string(),
            })
        }
    }
}
