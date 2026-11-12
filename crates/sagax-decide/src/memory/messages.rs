use super::{Fact, Pos, ZeroCapacity};

/// One message: a fact on its way from a sender to receivers.
///
/// A message is a plain value. After [`Outbox::due`] hands it out, the caller
/// may change it (for example refresh `pos` and `seen_at` from its current
/// fact) before routing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Message {
    /// The game's own kind number (becomes the fact's kind).
    pub kind: u32,
    /// What the message is about (becomes the fact's subject).
    pub subject: u32,
    /// Where the subject was.
    pub pos: Pos,
    /// When the subject was observed.
    pub seen_at: u32,
    /// Free for the game.
    pub strength: u32,
    /// Free for the game.
    pub data: u32,
    /// Who sent it (an id of the game).
    pub sender: u32,
    /// Not 0: [`Outbox::sender_gone`] keeps this message (a noise stays, a
    /// radio call is lost).
    pub keep_if_sender_gone: u8,
}

impl Message {
    /// The fact this message tells, with the given `source`.
    pub fn to_fact(&self, source: u8) -> Fact {
        Fact {
            subject: self.subject,
            kind: self.kind,
            pos: self.pos,
            seen_at: self.seen_at,
            strength: self.strength,
            data: self.data,
            source,
        }
    }
}

/// Why [`Outbox::post`] said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refused {
    /// The outbox holds as many messages as its capacity.
    Full,
    /// The last accepted post is younger than the cooldown.
    Cooldown,
}

#[derive(Clone, Copy, Debug)]
struct Posted {
    msg: Message,
    at: u32,
    delay: u32,
}

/// The messages one sender has posted and that are not yet delivered, with a
/// cooldown between posts. Fixed capacity.
#[derive(Clone, Debug)]
pub struct Outbox {
    items: Vec<Posted>,
    capacity: usize,
    last_post: Option<u32>,
}

impl Outbox {
    /// An empty outbox for at most `capacity` messages.
    pub fn new(capacity: usize) -> Result<Self, ZeroCapacity> {
        if capacity == 0 {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            items: Vec::with_capacity(capacity),
            capacity,
            last_post: None,
        })
    }

    /// Posts `msg` at time `now`; it becomes due after `delay`. Refused with
    /// [`Refused::Full`] if the outbox is full, else with
    /// [`Refused::Cooldown`] if the last accepted post is less than
    /// `cooldown` old. The first post never waits for the cooldown.
    pub fn post(
        &mut self,
        msg: Message,
        now: u32,
        delay: u32,
        cooldown: u32,
    ) -> Result<(), Refused> {
        if self.items.len() >= self.capacity {
            return Err(Refused::Full);
        }
        if let Some(last) = self.last_post
            && now.wrapping_sub(last) < cooldown
        {
            return Err(Refused::Cooldown);
        }
        self.items.push(Posted {
            msg,
            at: now,
            delay,
        });
        self.last_post = Some(now);
        Ok(())
    }

    /// Moves every message whose age is at least its own delay to the end of
    /// `out`, in post order.
    pub fn due(&mut self, now: u32, out: &mut Vec<Message>) {
        self.items.retain(|p| {
            let due = now.wrapping_sub(p.at) >= p.delay;
            if due {
                out.push(p.msg);
            }
            !due
        });
    }

    /// The sender is gone: drops every message not marked
    /// `keep_if_sender_gone`. The cooldown keeps running.
    pub fn sender_gone(&mut self) {
        self.items.retain(|p| p.msg.keep_if_sender_gone != 0);
    }

    /// Number of messages waiting.
    pub fn pending(&self) -> usize {
        self.items.len()
    }

    /// Drops all messages and forgets the last post (the next post is a
    /// first post again).
    pub fn clear(&mut self) {
        self.items.clear();
        self.last_post = None;
    }
}

/// The arrival time of a message sent at `now`: `now + base + distance *
/// per_unit`, wrapping.
pub fn arrival(now: u32, base: u32, distance: u32, per_unit: u32) -> u32 {
    now.wrapping_add(base)
        .wrapping_add(distance.wrapping_mul(per_unit))
}

/// The messages on their way to one receiver, each with its arrival time.
/// Fixed capacity.
#[derive(Clone, Debug)]
pub struct Inbox {
    items: Vec<(Message, u32)>,
    capacity: usize,
}

impl Inbox {
    /// An empty inbox for at most `capacity` messages.
    pub fn new(capacity: usize) -> Result<Self, ZeroCapacity> {
        if capacity == 0 {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            items: Vec::with_capacity(capacity),
            capacity,
        })
    }

    /// Adds `msg`, to be taken from `arrive_at` on. When full, the message
    /// that arrives latest (a tie: the later slot) is dropped if the new one
    /// arrives strictly earlier, and the new one is appended (true);
    /// otherwise the new one is refused (false). Arrival times are compared
    /// as plain numbers here, as there is no clock; keep them within one
    /// clock epoch.
    pub fn push(&mut self, msg: Message, arrive_at: u32) -> bool {
        if self.items.len() >= self.capacity {
            let mut latest = 0;
            for (i, it) in self.items.iter().enumerate().skip(1) {
                if it.1 >= self.items[latest].1 {
                    latest = i;
                }
            }
            if arrive_at >= self.items[latest].1 {
                return false;
            }
            self.items.remove(latest);
        }
        self.items.push((msg, arrive_at));
        true
    }

    /// Moves every message whose arrival time has come
    /// (`now.wrapping_sub(arrive_at) < 2^31`) to the end of `out`, in push
    /// order.
    pub fn take_due(&mut self, now: u32, out: &mut Vec<Message>) {
        self.items.retain(|(m, at)| {
            let due = now.wrapping_sub(*at) < 1 << 31;
            if due {
                out.push(*m);
            }
            !due
        });
    }

    /// Number of messages waiting.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if nothing waits.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drops all messages.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}
