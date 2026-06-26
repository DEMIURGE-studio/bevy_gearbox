//! Optional bevy_gauge integration — behind the `gauge` feature.
//!
//! [`Delay`] is an [`AttributeDerived`] component: syncs to it's entities
//! `Delay` attribute.

use std::time::Duration;

use bevy_gauge::prelude::{AttributeDerived, Attributes};

use crate::components::Delay;

impl AttributeDerived for Delay {
    fn should_update(&self, attrs: &Attributes) -> bool {
        let secs = attrs.value("Delay");
        secs > 0.0 && (self.duration.as_secs_f32() - secs).abs() > f32::EPSILON
    }

    fn update_from_attributes(&mut self, attrs: &Attributes) {
        let secs = attrs.value("Delay");
        if secs > 0.0 {
            self.duration = Duration::from_secs_f32(secs);
        }
    }
}

bevy_gauge::register_derived!(Delay);
