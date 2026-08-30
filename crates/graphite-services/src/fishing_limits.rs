/// Canonical maximum number of fish that may belong to one candidate Fishing cast.
///
/// This is a global Fishing invariant shared by Multicatch, School Bait, and capability/tension
/// evaluation. It is not owned by any one quantity modifier.
pub const MAX_FISH_PER_CAST: u8 = 5;
