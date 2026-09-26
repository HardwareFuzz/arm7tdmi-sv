mod ac_timing;
mod arm_toolchain;
mod campaign;
mod cdc_rdc;
#[cfg(test)]
mod characterization_contract;
mod compiler_program;
mod coverage_report;
#[cfg(test)]
mod debug_bridge_contract;
#[cfg(test)]
mod documentation_contract;
mod formal_report;
#[cfg(test)]
mod fpga_contract;
#[cfg(test)]
mod fpga_package_contract;
mod functional_coverage;
mod generation;
mod generic_soc_rom;
#[cfg(test)]
mod hdl_source_contract;
mod independent_lint;
#[cfg(test)]
mod integration_contracts;
mod mister_framework;
mod mister_framework_reports;
mod mutation_harness;
mod oss_cad_suite;
mod pinned_archive;
mod postfit_sim;
mod public_suite;
mod qemu;
mod quartus_reports;
mod random;
mod random_events;
mod reference;
mod regression;
mod release_evidence;
#[cfg(test)]
mod savestate_contract;
mod soak;
mod standalone_contract;
mod table7;
#[cfg(test)]
mod table7_contract;
#[cfg(test)]
mod table7_cross_contract;
mod traceability_report;
mod trm_coverage;
#[cfg(test)]
mod verification_contracts;

use std::env;
use std::path::PathBuf;

fn option(arguments: &[String], name: &str) -> Result<String, String> {
    let position = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("missing {name}"))?;
    arguments
        .get(position + 1)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn parsed<T: std::str::FromStr>(arguments: &[String], name: &str) -> Result<T, String> {
    option(arguments, name)?
        .parse::<T>()
        .map_err(|_| format!("invalid value for {name}"))
}

fn run(arguments: &[String]) -> Result<String, String> {
    let (command, args) = arguments
        .split_first()
        .ok_or_else(|| "expected differential, policy, qemu-expected, or word-hex".to_owned())?;
    match command.as_str() {
        "campaign" => {
            campaign::campaign(args)?;
            Ok(String::new())
        }
        "reference" => {
            reference::reference(args)?;
            Ok(String::new())
        }
        "table7-cross" => {
            table7::table7_cross(args)?;
            Ok(String::new())
        }
        "random-events" => {
            random_events::random_events(args)?;
            Ok(String::new())
        }
        "public-suite" => {
            public_suite::public_suite(args)?;
            Ok(String::new())
        }
        "functional-reference" => functional_coverage::functional_reference(args),
        "functional-coverage" => {
            functional_coverage::functional_coverage(args)?;
            Ok(String::new())
        }
        "soak" => {
            soak::soak(args)?;
            Ok(String::new())
        }
        "coverage-report" => {
            coverage_report::coverage_report(args)?;
            Ok(String::new())
        }
        "source-digest" => coverage_report::source_digest_command(args),
        "traceability-report" => {
            traceability_report::traceability_report(args)?;
            Ok(String::new())
        }
        "trm-coverage" => {
            trm_coverage::command(args)?;
            Ok(String::new())
        }
        "ac-timing" => {
            ac_timing::command(args)?;
            Ok(String::new())
        }
        "postfit-sim" => {
            postfit_sim::command(args)?;
            Ok(String::new())
        }
        "regression" => {
            regression::command(args)?;
            Ok(String::new())
        }
        "mister-framework" => {
            mister_framework::command(args)?;
            Ok(String::new())
        }
        "quartus-report-check" => {
            quartus_reports::report_command(args)?;
            Ok(String::new())
        }
        "quartus-option-characterization" => {
            quartus_reports::options_command(args)?;
            Ok(String::new())
        }
        "release-manifest" => release_evidence::manifest_command(),
        "release-package" => release_evidence::package_command(),
        "release-files-prefix" => release_evidence::files_prefix_command(),
        "release-evidence" => release_evidence::run_command(args),
        "release-validate" => release_evidence::validation_command(),
        "mutation-self-test" => mutation_harness::mutation_self_test(args),
        "generic-soc-rom-check" => generic_soc_rom::check(args),
        "arm-toolchain-install" => arm_toolchain::command(args),
        "compiler-program-build" => compiler_program::command(args),
        "oss-cad-suite-install" => oss_cad_suite::command(args),
        "formal-report" => formal_report::command(args),
        "independent-lint" => independent_lint::command(args),
        "cdc-rdc" => cdc_rdc::command(args),
        "standalone-contract" => {
            standalone_contract::command(args)?;
            Ok(String::new())
        }
        "differential" => {
            let seed = parsed(args, "--seed")?;
            let instructions = parsed(args, "--instructions")?;
            serde_json::to_string(&generation::differential(seed, instructions))
                .map_err(|error| error.to_string())
        }
        "policy" => {
            let seed = parsed(args, "--seed")?;
            let instructions = parsed(args, "--instructions")?;
            let endian = option(args, "--endian")?;
            let big_endian = match endian.as_str() {
                "big" => true,
                "little" => false,
                _ => return Err("--endian must be little or big".to_owned()),
            };
            serde_json::to_string(&generation::policy(seed, instructions, big_endian))
                .map_err(|error| error.to_string())
        }
        "qemu-expected" => {
            let trace = PathBuf::from(option(args, "--trace")?);
            let binary = PathBuf::from(option(args, "--binary")?);
            let begin = parsed::<u32>(args, "--begin")?;
            let end = parsed::<u32>(args, "--end")?;
            let expected = PathBuf::from(option(args, "--expected")?);
            let permitted_memory = PathBuf::from(option(args, "--permitted-memory")?);
            let program = PathBuf::from(option(args, "--program")?);
            let result = qemu::expected(
                &trace,
                &binary,
                begin,
                end,
                &expected,
                &permitted_memory,
                &program,
            )?;
            serde_json::to_string(&result).map_err(|error| error.to_string())
        }
        "word-hex" => {
            let binary = PathBuf::from(option(args, "--binary")?);
            let program = PathBuf::from(option(args, "--program")?);
            qemu::write_program_hex(&binary, &program)?;
            Ok("{}".to_owned())
        }
        _ => Err(format!("unknown command: {command}")),
    }
}

fn main() {
    let arguments: Vec<_> = env::args().skip(1).collect();
    match run(&arguments) {
        Ok(output) if !output.is_empty() => println!("{output}"),
        Ok(_) => {}
        Err(error) => {
            eprintln!("arm7-constrained-random: {error}");
            std::process::exit(1);
        }
    }
}
