//! Methods are namespaced by their self type.

use std::fmt;

pub struct Circle {
    pub radius: f64,
}

pub enum Shape {
    Circle(Circle),
    Square(f64),
    Rect { w: f64, h: f64 },
    Empty,
}

impl Shape {
    pub fn area(&self) -> f64 {
        match self {
            Shape::Circle(c) => 3.0 * c.radius * c.radius,
            Shape::Square(s) => s * s,
            Shape::Rect { w, h } if *w > 0.0 && *h > 0.0 => w * h,
            Shape::Rect { .. } => 0.0,
            Shape::Empty => 0.0,
        }
    }

    pub fn is_round(&self) -> bool {
        matches!(self, Shape::Circle(_))
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Shape::Circle(_) => "circle",
            Shape::Square(_) => "square",
            _ => "other",
        };
        write!(f, "{name}")
    }
}

pub struct Wrapper<T> {
    pub inner: Vec<T>,
}

impl<T: Clone> Wrapper<T> {
    pub fn first_or(&self, fallback: T) -> T {
        self.inner.first().cloned().unwrap_or(fallback)
    }
}

pub trait Describe {
    fn describe(&self) -> String;
}

impl Describe for &Circle {
    fn describe(&self) -> String {
        if self.radius > 1.0 {
            "big".to_string()
        } else {
            "small".to_string()
        }
    }
}

impl Describe for std::vec::Vec<Circle> {
    fn describe(&self) -> String {
        format!("{} circles", self.len())
    }
}
