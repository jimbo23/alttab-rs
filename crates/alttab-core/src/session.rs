use crate::{ItemKey, Rect, ReleaseAction};
use std::collections::VecDeque;
use std::ops::BitOr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers(pub u8);
impl Modifiers {
    pub const COMMAND: Self = Self(1);
    pub const OPTION: Self = Self(2);
    pub const CONTROL: Self = Self(4);
    pub const SHIFT: Self = Self(8);
    pub const CAPS_LOCK: Self = Self(16);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
impl BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    PendingShow,
    Visible,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionSource {
    #[default]
    Keyboard,
    Pointer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Render,
    ScheduleShow { generation: u64, deadline_ms: u64 },
    Hide { generation: u64 },
    Focus(ItemKey),
}
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Cycle { backward: bool, repeat: bool },
    Arrow { direction: Direction, repeat: bool },
    Hover(Option<ItemKey>),
    Pick(ItemKey),
    Modifiers(Modifiers),
    Accept,
    Cancel,
    Show { generation: u64 },
    SetRows(Vec<Rect>),
    Physical { serial: u64, action: PhysicalAction },
    EnterSearch,
    ExitSearch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicalAction {
    Tab { shift: bool, repeat: bool },
    ShiftPressed,
    Modifiers(Modifiers),
}

#[derive(Clone, Debug, Default)]
pub struct Session {
    phase: Phase,
    generation: u64,
    profile: usize,
    hold: Modifiers,
    release: ReleaseAction,
    items: Vec<ItemKey>,
    selected: Option<ItemKey>,
    hovered: Option<ItemKey>,
    selection_source: SelectionSource,
    attended: Option<ItemKey>,
    navigated: bool,
    rects: Vec<Rect>,
    physical: VecDeque<u64>,
    searching: bool,
}
impl Session {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn profile(&self) -> usize {
        self.profile
    }
    pub fn selected(&self) -> Option<ItemKey> {
        self.selected
    }
    pub fn selection_source(&self) -> SelectionSource {
        self.selection_source
    }
    pub fn items(&self) -> &[ItemKey] {
        &self.items
    }
    pub fn searching(&self) -> bool {
        self.searching
    }
    #[allow(clippy::too_many_arguments)]
    pub fn summon(
        &mut self,
        profile: usize,
        hold: Modifiers,
        release: ReleaseAction,
        items: Vec<ItemKey>,
        attended: Option<ItemKey>,
        now_ms: u64,
        delay_ms: u64,
    ) -> Vec<Effect> {
        let was_active = self.phase != Phase::Idle;
        let old_selection = self.selected;
        self.generation = self.generation.wrapping_add(1);
        self.profile = profile;
        self.hold = hold;
        self.release = release;
        self.attended = attended;
        self.items = unique(items);
        self.hovered = None;
        self.selection_source = SelectionSource::Keyboard;
        self.rects.clear();
        self.searching = release == ReleaseAction::Search;
        if self.items.is_empty() {
            self.phase = Phase::Idle;
            self.selected = None;
            self.navigated = false;
            return if was_active {
                vec![Effect::Hide {
                    generation: self.generation,
                }]
            } else {
                Vec::new()
            };
        }
        self.selected = if was_active
            && self.navigated
            && old_selection.is_some_and(|key| self.items.contains(&key))
        {
            old_selection
        } else {
            self.navigated = false;
            self.default_selection()
        };
        if was_active || delay_ms == 0 {
            self.phase = Phase::Visible;
            vec![Effect::Render]
        } else {
            self.phase = Phase::PendingShow;
            vec![Effect::ScheduleShow {
                generation: self.generation,
                deadline_ms: now_ms.saturating_add(delay_ms),
            }]
        }
    }
    fn default_selection(&self) -> Option<ItemKey> {
        let index = usize::from(
            !self.searching
                && self.items.len() > 1
                && self.attended.is_some_and(|key| self.items.contains(&key)),
        );
        self.items.get(index).copied()
    }
    pub fn reconcile(&mut self, items: Vec<ItemKey>) -> Vec<Effect> {
        if self.phase == Phase::Idle {
            return Vec::new();
        }
        let old_index = self
            .selected
            .and_then(|key| self.items.iter().position(|item| *item == key))
            .unwrap_or(0);
        self.items = unique(items);
        self.hovered = self.hovered.filter(|key| self.items.contains(key));
        self.rects.clear();
        if self.items.is_empty() {
            if self.searching {
                self.selected = None;
                return self.render();
            }
            return self.finish(false);
        }
        if !self.navigated {
            self.selected = self.default_selection();
        } else if !self.selected.is_some_and(|key| self.items.contains(&key)) {
            self.selected = self.items.get(old_index.min(self.items.len() - 1)).copied();
        }
        self.render()
    }
    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        if let Input::Physical { serial, action } = input {
            if self.physical.contains(&serial) {
                return Vec::new();
            }
            if self.physical.len() == 64 {
                self.physical.pop_front();
            }
            self.physical.push_back(serial);
            return self.handle(match action {
                PhysicalAction::Tab { shift, repeat } => Input::Cycle {
                    backward: shift,
                    repeat,
                },
                PhysicalAction::ShiftPressed => Input::Cycle {
                    backward: true,
                    repeat: false,
                },
                PhysicalAction::Modifiers(flags) => Input::Modifiers(flags),
            });
        }
        if self.phase == Phase::Idle {
            return Vec::new();
        }
        match input {
            Input::Accept => self.finish(true),
            Input::Cancel => self.finish(false),
            Input::Hover(key) if self.phase == Phase::Visible => {
                let Some(key) = key else {
                    self.hovered = None;
                    return Vec::new();
                };
                if self.hovered == Some(key) || !self.items.contains(&key) {
                    return Vec::new();
                }
                self.hovered = Some(key);
                self.selected = Some(key);
                self.navigated = true;
                self.selection_source = SelectionSource::Pointer;
                self.render()
            }
            Input::Pick(key) if self.items.contains(&key) => {
                self.selected = Some(key);
                self.finish(true)
            }
            Input::Modifiers(flags)
                if self.hold.0 != 0
                    && !flags.contains(self.hold)
                    && self.release == ReleaseAction::Focus
                    && !self.searching =>
            {
                self.finish(true)
            }
            Input::Show { generation }
                if generation == self.generation && self.phase == Phase::PendingShow =>
            {
                self.phase = Phase::Visible;
                vec![Effect::Render]
            }
            Input::Cycle { backward, repeat } => self.cycle(backward, repeat),
            Input::Arrow {
                direction: Direction::Left,
                repeat,
            } => self.cycle(true, repeat),
            Input::Arrow {
                direction: Direction::Right,
                repeat,
            } => self.cycle(false, repeat),
            Input::Arrow { direction, repeat } => self.vertical(direction, repeat),
            Input::SetRows(rects) => {
                if rects.len() == self.items.len() {
                    self.rects = rects;
                }
                Vec::new()
            }
            Input::EnterSearch => {
                self.searching = true;
                self.render()
            }
            Input::ExitSearch => {
                self.searching = false;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
    fn render(&self) -> Vec<Effect> {
        if self.phase == Phase::Visible {
            vec![Effect::Render]
        } else {
            Vec::new()
        }
    }
    fn cycle(&mut self, backward: bool, repeat: bool) -> Vec<Effect> {
        self.selection_source = SelectionSource::Keyboard;
        let Some(index) = self
            .selected
            .and_then(|key| self.items.iter().position(|item| *item == key))
        else {
            return Vec::new();
        };
        let next = if backward {
            if index == 0 {
                if repeat {
                    return Vec::new();
                }
                self.items.len() - 1
            } else {
                index - 1
            }
        } else if index + 1 == self.items.len() {
            if repeat {
                return Vec::new();
            }
            0
        } else {
            index + 1
        };
        self.selected = Some(self.items[next]);
        self.navigated = true;
        self.render()
    }
    fn vertical(&mut self, direction: Direction, repeat: bool) -> Vec<Effect> {
        self.selection_source = SelectionSource::Keyboard;
        let Some(index) = self
            .selected
            .and_then(|key| self.items.iter().position(|item| *item == key))
        else {
            return Vec::new();
        };
        let Some(current) = self.rects.get(index).copied() else {
            return self.cycle(direction == Direction::Up, repeat);
        };
        let down = direction == Direction::Down;
        let row_y = self
            .rects
            .iter()
            .map(|rect| rect.y)
            .filter(|y| if down { *y > current.y } else { *y < current.y })
            .min_by(|a, b| if down { a.total_cmp(b) } else { b.total_cmp(a) });
        let target_y = row_y.or_else(|| {
            if repeat {
                return None;
            }
            self.rects.iter().map(|rect| rect.y).min_by(|a, b| {
                if down {
                    a.total_cmp(b)
                } else {
                    b.total_cmp(a)
                }
            })
        });
        let Some(target_y) = target_y else {
            return Vec::new();
        };
        let target = self
            .rects
            .iter()
            .enumerate()
            .filter(|(_, rect)| rect.y == target_y)
            .min_by(|(ai, a), (bi, b)| {
                (a.center_x() - current.center_x())
                    .abs()
                    .total_cmp(&(b.center_x() - current.center_x()).abs())
                    .then(ai.cmp(bi))
            })
            .map(|(index, _)| index);
        if let Some(target) = target {
            self.selected = Some(self.items[target]);
            self.navigated = true;
        }
        self.render()
    }
    fn finish(&mut self, focus: bool) -> Vec<Effect> {
        let target = self.selected;
        self.phase = Phase::Idle;
        self.selected = None;
        self.hovered = None;
        self.selection_source = SelectionSource::Keyboard;
        self.items.clear();
        self.rects.clear();
        self.searching = false;
        self.navigated = false;
        let mut effects = vec![Effect::Hide {
            generation: self.generation,
        }];
        if focus {
            if let Some(target) = target {
                effects.push(Effect::Focus(target));
            }
        }
        effects
    }
}
fn unique(items: Vec<ItemKey>) -> Vec<ItemKey> {
    let mut seen = std::collections::BTreeSet::new();
    items.into_iter().filter(|key| seen.insert(*key)).collect()
}
