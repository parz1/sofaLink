use sofalink_simulator::{Scenario, check};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--help" || s == "help") {
        println!(
            "sofalink-simulator check [--json] [--scenario healthy|startup-fail|startup-timeout|audio-stall|video-stall|disconnect|stop-fail]"
        );
        return Ok(());
    }
    let mut scenarios = Scenario::ALL.to_vec();
    let mut json = false;
    let mut i = usize::from(args.first().is_some_and(|s| s == "check"));
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json = true,
            "--scenario" => {
                i += 1;
                let scenario = args
                    .get(i)
                    .and_then(|s| Scenario::parse(s))
                    .ok_or("unknown or missing scenario")?;
                scenarios = vec![scenario];
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
        i += 1;
    }
    for scenario in scenarios {
        let report = check(scenario)?;
        if json {
            println!("{}", serde_json::to_string(&report)?);
        } else {
            println!(
                "PASS {}: {:?} | resources released",
                scenario.id(),
                report.phases
            );
        }
    }
    Ok(())
}
