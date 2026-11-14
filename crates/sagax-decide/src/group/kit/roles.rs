/// Role bit of the default role list: fights from a distance. Counted in
/// the row column [`IN_RANGED`].
pub const ROLE_RANGED: u32 = 1;
/// Role bit of the default role list: hits an area (throws into crowds).
/// Counted in the row column [`IN_AREA`].
pub const ROLE_AREA: u32 = 2;

/// Built-in row column: active members with [`ROLE_RANGED`], the first
/// entry of the default role list ([`IN_ROLE_0`](super::super::IN_ROLE_0)).
pub const IN_RANGED: usize = super::super::squad::IN_ROLE_0;
/// Built-in row column: active members with [`ROLE_AREA`], the second entry
/// of the default role list ([`IN_ROLE_1`](super::super::IN_ROLE_1)).
pub const IN_AREA: usize = super::super::squad::IN_ROLE_1;
