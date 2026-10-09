//! Tunable settings: names, limits, CLI args and console-command parsing.
//! Pure (no OS calls), so it is testable on any platform.
//!
//! Every parameter is described once in `Param`; the help text, clamping,
//! CLI order and console parsing are all derived from that single table.

use std::iter::Peekable;
use std::str::SplitWhitespace;

use crate::brush::{LazyBrush, Point, Spring};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Radius,
    Friction,
    Stiffness,
    Damping,
    Stretch,
}

impl Param {
    /// Also the order of the positional CLI arguments.
    pub const ALL: [Param; 5] = [
        Param::Radius,
        Param::Friction,
        Param::Stiffness,
        Param::Damping,
        Param::Stretch,
    ];
    pub const COUNT: usize = Self::ALL.len();

    pub const fn name(self) -> &'static str {
        match self {
            Param::Radius => "radius",
            Param::Friction => "friction",
            Param::Stiffness => "stiffness",
            Param::Damping => "damping",
            Param::Stretch => "stretch",
        }
    }

    pub const fn alias(self) -> &'static str {
        match self {
            Param::Radius => "r",
            Param::Friction => "f",
            Param::Stiffness => "s",
            Param::Damping => "d",
            Param::Stretch => "x",
        }
    }

    /// Inclusive (min, max).
    pub const fn range(self) -> (f64, f64) {
        match self {
            Param::Radius => (0.0, 300.0),
            Param::Friction => (0.0, 1.0),
            Param::Stiffness => (1.0, 2000.0),
            Param::Damping => (0.0, 300.0),
            Param::Stretch => (1.0, 1.0e6),
        }
    }

    pub const fn help(self) -> &'static str {
        match self {
            Param::Radius => "lazy area size in px",
            Param::Friction => "1 = brush never moves",
            Param::Stiffness => "higher = snappier pull",
            Param::Damping => "lower = builds speed more freely",
            Param::Stretch => "px; lower = more nonlinear",
        }
    }

    pub fn clamp(self, v: f64) -> f64 {
        let (lo, hi) = self.range();
        v.clamp(lo, hi)
    }

    pub fn from_word(word: &str) -> Option<Param> {
        Self::ALL
            .into_iter()
            .find(|p| word == p.name() || word == p.alias())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub radius: f64,
    pub friction: f64,
    pub spring: Spring,
}

impl Settings {
    pub const DEFAULT: Settings = Settings {
        radius: 40.0,
        friction: 0.0,
        spring: Spring::DEFAULT,
    };

    pub fn get(&self, p: Param) -> f64 {
        match p {
            Param::Radius => self.radius,
            Param::Friction => self.friction,
            Param::Stiffness => self.spring.stiffness,
            Param::Damping => self.spring.damping,
            Param::Stretch => self.spring.stretch_ref,
        }
    }

    /// Clamps to the parameter's range; non-finite values are ignored.
    pub fn set(&mut self, p: Param, v: f64) {
        if !v.is_finite() {
            return;
        }
        let v = p.clamp(v);
        match p {
            Param::Radius => self.radius = v,
            Param::Friction => self.friction = v,
            Param::Stiffness => self.spring.stiffness = v,
            Param::Damping => self.spring.damping = v,
            Param::Stretch => self.spring.stretch_ref = v,
        }
    }

    pub fn apply(&mut self, u: &Update) {
        for p in Param::ALL {
            if let Some(v) = u.get(p) {
                self.set(p, v);
            }
        }
    }

    /// Positional args in `Param::ALL` order; missing or invalid ones keep the default.
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut s = Self::DEFAULT;
        for (p, a) in Param::ALL.into_iter().zip(args) {
            if let Ok(v) = a.parse::<f64>() {
                s.set(p, v);
            }
        }
        s
    }

    pub fn brush(&self, at: Point) -> LazyBrush {
        LazyBrush::new(self.radius, at)
            .with_friction(self.friction)
            .with_spring(self.spring)
    }

    pub fn summary(&self) -> String {
        format!(
            "radius {}px, friction {}, stiffness {}, damping {}, stretch {}",
            self.radius,
            self.friction,
            self.spring.stiffness,
            self.spring.damping,
            self.spring.stretch_ref
        )
    }
}

/// A batch of changes (one slot per `Param`), merged from console commands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Update([Option<f64>; Param::COUNT]);

impl Update {
    pub const NONE: Update = Update([None; Param::COUNT]);

    pub fn get(&self, p: Param) -> Option<f64> {
        self.0[p as usize]
    }

    pub fn set(&mut self, p: Param, v: f64) {
        self.0[p as usize] = Some(v);
    }

    /// Values in `other` win.
    pub fn merge(&mut self, other: Update) {
        for (a, b) in self.0.iter_mut().zip(other.0) {
            *a = b.or(*a);
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Command {
    pub update: Update,
    pub quit: bool,
    /// Words that were not understood, for the caller to report.
    pub ignored: Vec<String>,
}

/// Parse one console line, e.g. `r 60 f 0.3` or `stiffness 250 damping 18`.
pub fn parse_command(line: &str) -> Command {
    let mut cmd = Command::default();
    let mut it = line.split_whitespace().peekable();
    while let Some(word) = it.next() {
        if word == "quit" || word == "q" {
            cmd.quit = true;
        } else if let Some(p) = Param::from_word(word) {
            match take_number(&mut it) {
                Some(v) => cmd.update.set(p, p.clamp(v)),
                None => cmd.ignored.push(format!("{word} (needs a number)")),
            }
        } else {
            cmd.ignored.push(word.to_string());
        }
    }
    cmd
}

/// Consumes the next token only if it is a finite number.
fn take_number(it: &mut Peekable<SplitWhitespace<'_>>) -> Option<f64> {
    let v = it.peek()?.parse::<f64>().ok().filter(|v| v.is_finite())?;
    it.next();
    Some(v)
}

pub fn print_help() {
    println!("CLI: lazy-mouse [radius] [friction] [stiffness] [damping] [stretch]");
    println!("commands (e.g. \"r 60 s 250 d 18\"):");
    for p in Param::ALL {
        let (lo, hi) = p.range();
        println!(
            "  {:<16}{:<16}{}",
            format!("{}|{}", p.name(), p.alias()),
            format!("{lo} .. {hi}"),
            p.help()
        );
    }
    println!("  quit|q");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_long_names() {
        let c = parse_command("r 60 friction 0.3 s 250 d 18 x 80");
        assert_eq!(c.update.get(Param::Radius), Some(60.0));
        assert_eq!(c.update.get(Param::Friction), Some(0.3));
        assert_eq!(c.update.get(Param::Stiffness), Some(250.0));
        assert_eq!(c.update.get(Param::Damping), Some(18.0));
        assert_eq!(c.update.get(Param::Stretch), Some(80.0));
        assert!(c.ignored.is_empty() && !c.quit);
    }

    #[test]
    fn clamps_to_the_range() {
        let c = parse_command("r 9999 f -4 s 0");
        assert_eq!(c.update.get(Param::Radius), Some(300.0));
        assert_eq!(c.update.get(Param::Friction), Some(0.0));
        assert_eq!(c.update.get(Param::Stiffness), Some(1.0));
    }

    #[test]
    fn missing_numbers_do_not_swallow_the_next_command() {
        let c = parse_command("r f 0.3");
        assert_eq!(c.update.get(Param::Radius), None);
        assert_eq!(c.update.get(Param::Friction), Some(0.3));
        assert_eq!(c.ignored, ["r (needs a number)"]);
    }

    #[test]
    fn unknown_words_are_reported() {
        let c = parse_command("bogus 3");
        assert_eq!(c.update, Update::NONE);
        assert_eq!(c.ignored, ["bogus", "3"]);
    }

    #[test]
    fn quit_is_detected() {
        assert!(parse_command("q").quit);
        assert!(parse_command("quit").quit);
    }

    #[test]
    fn from_args_is_positional_and_falls_back() {
        let s = Settings::from_args(["60", "x", "250"].map(String::from));
        assert_eq!(s.radius, 60.0);
        assert_eq!(s.friction, 0.0);
        assert_eq!(s.spring.stiffness, 250.0);
        assert_eq!(s.spring.damping, Spring::DEFAULT.damping);
    }

    #[test]
    fn merge_keeps_the_latest_value_per_slot() {
        let mut a = parse_command("r 10 f 0.1").update;
        a.merge(parse_command("r 20 d 5").update);
        assert_eq!(a.get(Param::Radius), Some(20.0));
        assert_eq!(a.get(Param::Friction), Some(0.1));
        assert_eq!(a.get(Param::Damping), Some(5.0));
    }

    #[test]
    fn apply_updates_settings() {
        let mut s = Settings::DEFAULT;
        s.apply(&parse_command("r 99 x 1e9").update);
        assert_eq!(s.radius, 99.0);
        assert_eq!(s.get(Param::Stretch), 1.0e6);
    }
}
