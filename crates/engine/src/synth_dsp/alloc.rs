//! The voice allocator: which voice plays what, for keys and the sustain pedal.
//!
//! A pure state machine: it doesn't touch audio. A model feeds it key and pedal events and acts on
//! the [`VoiceAction`]s it hands back, in order, at the sample the event arrived. Three modes:
//!
//! - **Poly.** Each key takes [`set_unison`](VoiceAllocator::set_unison) voices. With none free it
//!   steals: the longest-released first, then the oldest held.
//! - **Mono.** One note at a time, chosen from the keys down by [`Priority`]. Single trigger plays
//!   other keys legato; multi trigger restarts the envelopes on each new note. All the unison voices
//!   move together.
//!
//! **Stealing without a click** is up to the voice, which gets [`VoiceAction::Start`] with
//! `stolen` set while it is still sounding. A free-running analog-style voice needs nothing more:
//! the oscillators' phases and the filter carry on and the envelopes attack from their current
//! level (see `env.rs`), which is continuous. That is also what analog polysynths do. A voice whose
//! phase does jump, such as a sampler's, should fade: keep a few spare voices beyond its
//! `polyphony`, let the stolen one fade over a few milliseconds and start the note on a spare.
//!
//! No allocation: the state is inline, sized by `V`, and actions go to a closure.

/// Which key sounds when several are down in mono mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    Last,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceMode {
    Poly,
    Mono {
        priority: Priority,
        /// Restart the envelopes on every new note, not only after all keys were up.
        multi_trigger: bool,
    },
}

/// What a voice is told to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VoiceAction {
    /// Sound a note and start the envelopes. `stolen`: it is still sounding something else.
    /// `unison` is the voice's place in its group (index, size), for its detune and pan.
    Start { voice: usize, note: u8, velocity: f32, stolen: bool, unison: (u8, u8) },
    /// Change the note without restarting the envelopes (mono, legato).
    Glide { voice: usize, note: u8 },
    /// The key is up: start the release.
    Release { voice: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum VoiceState {
    #[default]
    Free,
    Held,
    /// The key is up, but the pedal is down.
    Pedal,
    Released,
}

#[derive(Debug, Clone, Copy, Default)]
struct VoiceSlot {
    state: VoiceState,
    note: u8,
    /// When it started or was let go; lower is older.
    age: u64,
}

/// The allocator for `V` voices.
#[derive(Debug, Clone, Copy)]
pub struct VoiceAllocator<const V: usize> {
    mode: VoiceMode,
    unison: usize,
    polyphony: usize,
    slots: [VoiceSlot; V],
    clock: u64,
    pedal: bool,
    /// Keys down in the order pressed, with their velocity. A key let go under the pedal stays
    /// here until it comes up (`held_up`).
    held: [(u8, f32); 128],
    held_len: usize,
    held_up: [bool; 128],
    /// The note the mono voices are playing.
    mono_note: Option<u8>,
}

impl<const V: usize> VoiceAllocator<V> {
    pub fn new() -> Self {
        VoiceAllocator {
            mode: VoiceMode::Mono { priority: Priority::Last, multi_trigger: false },
            unison: 1,
            polyphony: V,
            slots: [VoiceSlot::default(); V],
            clock: 0,
            pedal: false,
            held: [(0, 0.0); 128],
            held_len: 0,
            held_up: [false; 128],
            mono_note: None,
        }
    }

    /// Changes the mode. Notes already sounding carry on and end with their keys, except that going
    /// to mono or to another size of group releases them, to start from a clean slate.
    pub fn set_mode(&mut self, mode: VoiceMode, unison: usize, polyphony: usize, mut act: impl FnMut(VoiceAction)) {
        let unison = unison.clamp(1, V);
        let polyphony = polyphony.clamp(1, V);
        if mode == self.mode && unison == self.unison && polyphony == self.polyphony {
            return;
        }
        // Only what changes the mapping lets go of the notes: a priority change alone doesn't.
        let same_shape = matches!((mode, self.mode), (VoiceMode::Mono { .. }, VoiceMode::Mono { .. })) && unison == self.unison;
        if !same_shape {
            self.release_all(&mut act);
            self.held_len = 0;
            self.held_up = [false; 128];
            self.mono_note = None;
        }
        self.mode = mode;
        self.unison = unison;
        self.polyphony = polyphony;
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn release_voice(&mut self, voice: usize, act: &mut impl FnMut(VoiceAction)) {
        let now = self.tick();
        let s = &mut self.slots[voice];
        s.state = VoiceState::Released;
        s.age = now;
        act(VoiceAction::Release { voice });
    }

    /// Lets go of everything that is sounding: keys and pedal forgotten, tails ring out.
    pub fn release_all(&mut self, mut act: impl FnMut(VoiceAction)) {
        for v in 0..V {
            if matches!(self.slots[v].state, VoiceState::Held | VoiceState::Pedal) {
                self.release_voice(v, &mut act);
            }
        }
        self.held_len = 0;
        self.held_up = [false; 128];
        self.mono_note = None;
        self.pedal = false;
    }

    /// Forgets everything at once, for a voice bank that was just cleared.
    pub fn reset(&mut self) {
        self.slots = [VoiceSlot::default(); V];
        self.clock = 0;
        self.pedal = false;
        self.held_len = 0;
        self.held_up = [false; 128];
        self.mono_note = None;
    }

    /// A voice's envelopes have finished: it is free.
    pub fn voice_finished(&mut self, voice: usize) {
        if self.slots[voice].state == VoiceState::Released {
            self.slots[voice] = VoiceSlot::default();
        }
    }

    /// Picks the voice to sound a new note on: a free one, then one with the same note ringing
    /// out, then the longest released, then the oldest held.
    fn take_voice(&self, note: u8, taken: &[bool; V]) -> (usize, bool) {
        let open = |v: &usize| !taken[*v] && *v < self.pool();
        if let Some(v) = (0..V).filter(open).find(|&v| self.slots[v].state == VoiceState::Free) {
            return (v, false);
        }
        let released = |v: &usize| self.slots[*v].state == VoiceState::Released;
        if let Some(v) = (0..V).filter(open).filter(released).find(|&v| self.slots[v].note == note) {
            return (v, true);
        }
        if let Some(v) = (0..V).filter(open).filter(released).min_by_key(|&v| self.slots[v].age) {
            return (v, true);
        }
        ((0..V).filter(open).min_by_key(|&v| self.slots[v].age).unwrap_or(0), true)
    }

    /// How many voices may play: whole groups of `unison` up to the polyphony.
    fn pool(&self) -> usize {
        match self.mode {
            VoiceMode::Mono { .. } => self.unison,
            VoiceMode::Poly => (self.polyphony / self.unison).max(1) * self.unison,
        }
        .min(V)
    }

    fn start_group(&mut self, note: u8, velocity: f32, act: &mut impl FnMut(VoiceAction)) {
        let size = self.unison;
        let mut taken = [false; V];
        for i in 0..size {
            let (voice, stolen) = self.take_voice(note, &taken);
            taken[voice] = true;
            let age = self.tick();
            self.slots[voice] = VoiceSlot { state: VoiceState::Held, note, age };
            act(VoiceAction::Start { voice, note, velocity, stolen, unison: (i as u8, size as u8) });
        }
    }

    fn add_held(&mut self, note: u8, velocity: f32) {
        self.remove_held(note);
        if self.held_len < 128 {
            self.held[self.held_len] = (note, velocity);
            self.held_len += 1;
        }
    }

    fn remove_held(&mut self, note: u8) {
        if let Some(k) = self.held[..self.held_len].iter().position(|h| h.0 == note) {
            self.held.copy_within(k + 1..self.held_len, k);
            self.held_len -= 1;
        }
        self.held_up[note as usize & 127] = false;
    }

    /// The key a mono voice should sound from those down.
    fn mono_choice(&self, priority: Priority) -> Option<(u8, f32)> {
        let keys = &self.held[..self.held_len];
        match priority {
            Priority::Last => keys.last().copied(),
            Priority::Low => keys.iter().copied().min_by_key(|h| h.0),
            Priority::High => keys.iter().copied().max_by_key(|h| h.0),
        }
    }

    /// Moves mono's voices to whichever note should be sounding now.
    fn mono_update(&mut self, priority: Priority, multi_trigger: bool, retrigger_same: bool, act: &mut impl FnMut(VoiceAction)) {
        let want = self.mono_choice(priority);
        match (self.mono_note, want) {
            (None, Some((note, velocity))) => {
                self.mono_note = Some(note);
                self.start_group(note, velocity, act);
            }
            (Some(_), None) => {
                self.mono_note = None;
                for v in 0..V {
                    if matches!(self.slots[v].state, VoiceState::Held | VoiceState::Pedal) {
                        self.release_voice(v, act);
                    }
                }
            }
            (Some(cur), Some((note, velocity))) if note != cur || retrigger_same => {
                self.mono_note = Some(note);
                if multi_trigger {
                    self.start_group(note, velocity, act);
                } else {
                    for v in 0..V {
                        if self.slots[v].state == VoiceState::Held {
                            self.slots[v].note = note;
                            act(VoiceAction::Glide { voice: v, note });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// A key went down. `velocity` is 0..1.
    pub fn note_on(&mut self, note: u8, velocity: f32, mut act: impl FnMut(VoiceAction)) {
        let note = note & 127;
        match self.mode {
            VoiceMode::Mono { priority, multi_trigger } => {
                self.add_held(note, velocity);
                self.mono_update(priority, multi_trigger, false, &mut act);
            }
            VoiceMode::Poly => {
                // The same key again under the pedal, or a repeat without its note-off, retriggers
                // the same voices rather than piling up.
                let mut retrig = false;
                for v in 0..V {
                    if self.slots[v].note == note && matches!(self.slots[v].state, VoiceState::Pedal | VoiceState::Held) {
                        let age = self.tick();
                        self.slots[v].state = VoiceState::Held;
                        self.slots[v].age = age;
                        retrig = true;
                        // Its place in the group stays as it was; the voice keeps its detune.
                        act(VoiceAction::Start { voice: v, note, velocity, stolen: true, unison: (u8::MAX, u8::MAX) });
                    }
                }
                if !retrig {
                    self.start_group(note, velocity, &mut act);
                }
            }
        }
    }

    /// A key came up.
    pub fn note_off(&mut self, note: u8, mut act: impl FnMut(VoiceAction)) {
        let note = note & 127;
        match self.mode {
            VoiceMode::Mono { priority, multi_trigger } => {
                if self.pedal {
                    // Stays in the list until the pedal lets go.
                    self.held_up[note as usize] = true;
                    return;
                }
                self.remove_held(note);
                self.mono_update(priority, multi_trigger, false, &mut act);
            }
            VoiceMode::Poly => {
                for v in 0..V {
                    if self.slots[v].note == note && self.slots[v].state == VoiceState::Held {
                        if self.pedal {
                            self.slots[v].state = VoiceState::Pedal;
                        } else {
                            self.release_voice(v, &mut act);
                        }
                    }
                }
            }
        }
    }

    /// The sustain pedal.
    pub fn set_pedal(&mut self, down: bool, mut act: impl FnMut(VoiceAction)) {
        if self.pedal == down {
            return;
        }
        self.pedal = down;
        if down {
            return;
        }
        match self.mode {
            VoiceMode::Mono { priority, multi_trigger } => {
                let mut any = false;
                for n in 0..128 {
                    if self.held_up[n] {
                        self.remove_held(n as u8);
                        any = true;
                    }
                }
                if any {
                    self.mono_update(priority, multi_trigger, false, &mut act);
                }
            }
            VoiceMode::Poly => {
                for v in 0..V {
                    if self.slots[v].state == VoiceState::Pedal {
                        self.release_voice(v, &mut act);
                    }
                }
            }
        }
    }

    /// Whether a voice is sounding or may be (a key or the pedal holds it, or it's released and
    /// hasn't reported finished).
    pub fn is_busy(&self, voice: usize) -> bool {
        self.slots[voice].state != VoiceState::Free
    }
}

impl<const V: usize> Default for VoiceAllocator<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// A unison voice's place in its group as a detune (-1..1, widest for the outer voices) and a pan
/// (-1..1, alternating sides): voices spread evenly, and a lone voice sits in the middle.
pub fn unison_spread(index: u8, size: u8) -> (f32, f32) {
    if size <= 1 || index >= size {
        return (0.0, 0.0);
    }
    let x = 2.0 * index as f32 / (size - 1) as f32 - 1.0;
    let pan = if index & 1 == 0 { x.abs() } else { -x.abs() };
    (x, pan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use VoiceAction::*;

    fn collect<const V: usize>(f: impl FnOnce(&mut VoiceAllocator<V>, &mut dyn FnMut(VoiceAction)), a: &mut VoiceAllocator<V>) -> Vec<VoiceAction> {
        let mut out = Vec::new();
        f(a, &mut |x| out.push(x));
        out
    }

    fn on<const V: usize>(a: &mut VoiceAllocator<V>, note: u8) -> Vec<VoiceAction> {
        collect(|a, act| a.note_on(note, 0.5, act), a)
    }

    fn off<const V: usize>(a: &mut VoiceAllocator<V>, note: u8) -> Vec<VoiceAction> {
        collect(|a, act| a.note_off(note, act), a)
    }

    fn poly<const V: usize>(unison: usize) -> VoiceAllocator<V> {
        let mut a = VoiceAllocator::<V>::new();
        a.set_mode(VoiceMode::Poly, unison, V, |_| {});
        a
    }

    fn mono(priority: Priority, multi_trigger: bool) -> VoiceAllocator<4> {
        let mut a = VoiceAllocator::<4>::new();
        a.set_mode(VoiceMode::Mono { priority, multi_trigger }, 1, 4, |_| {});
        a
    }

    #[test]
    fn poly_uses_free_voices_then_steals_released_before_held() {
        let mut a = poly::<3>(1);
        assert!(matches!(on(&mut a, 60)[..], [Start { voice: 0, note: 60, stolen: false, .. }]));
        assert!(matches!(on(&mut a, 62)[..], [Start { voice: 1, note: 62, stolen: false, .. }]));
        assert!(matches!(on(&mut a, 64)[..], [Start { voice: 2, note: 64, stolen: false, .. }]));
        // Let go of the middle one: it goes first.
        assert_eq!(off(&mut a, 62), vec![Release { voice: 1 }]);
        assert!(matches!(on(&mut a, 65)[..], [Start { voice: 1, note: 65, stolen: true, .. }]));
        // Then the oldest held.
        assert!(matches!(on(&mut a, 67)[..], [Start { voice: 0, note: 67, stolen: true, .. }]));
        // A note-off for what was stolen does nothing.
        assert_eq!(off(&mut a, 60), vec![]);
        assert_eq!(off(&mut a, 67), vec![Release { voice: 0 }]);
        a.voice_finished(0);
        assert!(matches!(on(&mut a, 70)[..], [Start { voice: 0, stolen: false, .. }]));
    }

    #[test]
    fn sustain_pedal_holds_until_it_lifts() {
        let mut a = poly::<4>(1);
        on(&mut a, 60);
        collect(|a, act| a.set_pedal(true, act), &mut a);
        assert_eq!(off(&mut a, 60), vec![]);
        // Playing it again uses the same voice.
        assert!(matches!(on(&mut a, 60)[..], [Start { voice: 0, .. }]));
        assert_eq!(off(&mut a, 60), vec![]);
        assert_eq!(collect(|a, act| a.set_pedal(false, act), &mut a), vec![Release { voice: 0 }]);
    }

    #[test]
    fn low_note_priority_keeps_the_lowest_and_returns_to_it() {
        let mut a = mono(Priority::Low, false);
        assert!(matches!(on(&mut a, 60)[..], [Start { note: 60, .. }]));
        assert_eq!(on(&mut a, 67), vec![], "a higher key changes nothing");
        assert_eq!(on(&mut a, 55), vec![Glide { voice: 0, note: 55 }]);
        assert_eq!(off(&mut a, 67), vec![]);
        assert_eq!(off(&mut a, 55), vec![Glide { voice: 0, note: 60 }]);
        assert_eq!(off(&mut a, 60), vec![Release { voice: 0 }]);
        assert!(matches!(on(&mut a, 48)[..], [Start { note: 48, .. }]), "all keys were up: a fresh note");
    }

    #[test]
    fn last_and_high_priority_and_multi_trigger() {
        let mut a = mono(Priority::Last, false);
        on(&mut a, 60);
        assert_eq!(on(&mut a, 55), vec![Glide { voice: 0, note: 55 }]);
        assert_eq!(off(&mut a, 55), vec![Glide { voice: 0, note: 60 }]);

        let mut a = mono(Priority::High, false);
        on(&mut a, 60);
        assert_eq!(on(&mut a, 55), vec![]);
        assert_eq!(on(&mut a, 62), vec![Glide { voice: 0, note: 62 }]);

        let mut a = mono(Priority::Last, true);
        on(&mut a, 60);
        assert!(matches!(on(&mut a, 62)[..], [Start { voice: 0, note: 62, stolen: true, .. }]));
    }

    #[test]
    fn pedal_in_mono_keeps_the_note_until_it_lifts() {
        let mut a = mono(Priority::Last, false);
        on(&mut a, 60);
        collect(|a, act| a.set_pedal(true, act), &mut a);
        assert_eq!(off(&mut a, 60), vec![]);
        assert_eq!(collect(|a, act| a.set_pedal(false, act), &mut a), vec![Release { voice: 0 }]);
    }

    #[test]
    fn unison_moves_together_and_steals_whole_groups() {
        let mut a = poly::<4>(2);
        let s = on(&mut a, 60);
        assert_eq!(s.len(), 2);
        assert!(matches!(s[1], Start { voice: 1, unison: (1, 2), .. }));
        on(&mut a, 62);
        let stolen = on(&mut a, 64);
        assert!(stolen.iter().all(|x| matches!(x, Start { stolen: true, .. })), "{stolen:?}");
        assert_eq!(off(&mut a, 62).len(), 2);

        let mut m = VoiceAllocator::<4>::new();
        m.set_mode(VoiceMode::Mono { priority: Priority::Last, multi_trigger: false }, 3, 4, |_| {});
        assert_eq!(on(&mut m, 60).len(), 3);
        assert_eq!(on(&mut m, 61).iter().filter(|x| matches!(x, Glide { .. })).count(), 3);
        assert_eq!(off(&mut m, 61).len(), 3, "back to 60");
        assert_eq!(off(&mut m, 60).iter().filter(|x| matches!(x, Release { .. })).count(), 3);
    }

    #[test]
    fn polyphony_caps_the_pool_in_whole_groups() {
        let mut a = VoiceAllocator::<8>::new();
        a.set_mode(VoiceMode::Poly, 2, 5, |_| {});
        for n in 0..3 {
            for x in on(&mut a, 60 + n) {
                let Start { voice, .. } = x else { panic!() };
                assert!(voice < 4, "{voice}: two groups of 2 fit in 5 voices");
            }
        }
    }

    #[test]
    fn spread_is_even_and_centred() {
        assert_eq!(unison_spread(0, 1), (0.0, 0.0));
        assert_eq!(unison_spread(0, 2).0, -1.0);
        assert_eq!(unison_spread(1, 2).0, 1.0);
        let pans: Vec<f32> = (0..4).map(|i| unison_spread(i, 4).1).collect();
        assert_eq!(pans.iter().filter(|p| **p > 0.0).count(), 2);
    }
}
