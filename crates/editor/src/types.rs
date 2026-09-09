use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub u64);

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Heuristic decode:
        // - If high 32 bits are nonzero, or low 32 bits look like !index (large), treat as to_bits()
        // - Otherwise treat the value as a plain row index with generation 0
        let raw = self.0;
        let low = (raw & 0xFFFF_FFFF) as u32;
        let high = ((raw >> 32) & 0xFFFF_FFFF) as u32;
        let looks_like_bits = high != 0 || low > 0x7FFF_FFFF;
        if looks_like_bits {
            let index = !low;
            let generation = high;
            write!(f, "{}v{}", index, generation)
        } else {
            write!(f, "{}v0", low)
        }
    }
}


