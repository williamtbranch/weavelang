fn main() {
    let raw = "Line 1\nLine 2";
    println!("Raw: {:?}", raw);
    for line in raw.lines() {
        println!("Line: {:?}", line);
    }
}
