//! Compares the ways to put factors together on typical situations.
//! `cargo run -p sagax-decide --example compare`

use sagax_decide::{Combine, ONE, combine, pull};

const HOW: [(Combine, &str); 5] = [
    (Combine::Product, "product"),
    (Combine::GeometricMean, "geo mean"),
    (Combine::Minimum, "minimum"),
    (Combine::Average, "average"),
    (Combine::Median, "median"),
];

fn f(v: f64) -> u32 {
    (v * f64::from(ONE)).round() as u32
}

fn show(title: &str, options: &[(&str, &[f64])]) {
    println!("\n{title}");
    print!("  {:<28}", "");
    for (_, name) in HOW {
        print!("{name:>10}");
    }
    println!();
    for (label, values) in options {
        print!("  {label:<28}");
        for (how, _) in HOW {
            let mut v: Vec<u32> = values.iter().map(|&x| f(x)).collect();
            print!("{:>10.3}", f64::from(combine(how, &mut v)) / f64::from(ONE));
        }
        println!();
    }
}

fn main() {
    show(
        "1. Many good factors against one fair factor",
        &[("A: five times 0.9", &[0.9; 5]), ("B: one 0.8", &[0.8])],
    );
    show(
        "2. One very bad factor",
        &[
            ("A: 1, 1, 1, 0.1", &[1.0, 1.0, 1.0, 0.1]),
            ("B: 0.5 four times", &[0.5; 4]),
        ],
    );
    show(
        "3. A factor that rules out",
        &[
            ("A: 1, 1, 0 (no line of sight)", &[1.0, 1.0, 0.0]),
            ("B: 0.3, 0.3, 0.3", &[0.3; 3]),
        ],
    );
    show(
        "4. A single outlier",
        &[
            ("A: 0.9, 0.9, 0.9, 0.05", &[0.9, 0.9, 0.9, 0.05]),
            ("B: 0.6 four times", &[0.6; 4]),
        ],
    );
    println!("\nPull on a factor of 0.4 (1 = as it is):");
    for p in [0.0, 0.25, 0.5, 1.0, 1.5, 2.0] {
        println!(
            "  pull {p:<4} -> {:.3}",
            f64::from(pull(f(0.4), f(p))) / f64::from(ONE)
        );
    }
}
