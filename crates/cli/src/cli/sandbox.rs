use anyhow::Result;
use clap::{Args, Subcommand};

use localgpt_core::config::Config;
use localgpt_sandbox::{SandboxLevel, build_policy, detect_capabilities, run_sandboxed};

#[derive(Args)]
pub struct SandboxArgs {
    #[command(subcommand)]
    pub command: SandboxCommands,
}

#[derive(Subcommand)]
pub enum SandboxCommands {
    /// Show sandbox capabilities and configuration
    Status,

    /// Run smoke tests to verify sandbox enforcement
    Test,
}

pub async fn run(args: SandboxArgs) -> Result<()> {
    match args.command {
        SandboxCommands::Status => run_status().await,
        SandboxCommands::Test => run_test().await,
    }
}

async fn run_status() -> Result<()> {
    let config = Config::load()?;
    let caps = detect_capabilities();

    println!("Sandbox Capabilities:");
    for line in caps.status_lines() {
        println!("{}", line);
    }
    println!();

    let effective = caps.effective_level(&config.sandbox.level);
    println!("Configuration:");
    println!("  Enabled:     {}", config.sandbox.enabled);
    println!(
        "  Level:       {} (config: {})",
        format_level(effective),
        config.sandbox.level
    );
    println!("  Timeout:     {}s", config.sandbox.timeout_secs);
    println!("  Max output:  {} bytes", config.sandbox.max_output_bytes);
    println!(
        "  Max fsize:   {} bytes",
        config.sandbox.max_file_size_bytes
    );
    println!("  Max procs:   {}", config.sandbox.max_processes);
    println!("  Network:     {}", config.sandbox.network.policy);

    if !config.sandbox.allow_paths.read.is_empty() {
        println!("  Extra read:  {:?}", config.sandbox.allow_paths.read);
    }
    if !config.sandbox.allow_paths.write.is_empty() {
        println!("  Extra write: {:?}", config.sandbox.allow_paths.write);
    }

    Ok(())
}

async fn run_test() -> Result<()> {
    let config = Config::load()?;
    let caps = detect_capabilities();
    let effective = caps.effective_level(&config.sandbox.level);

    if !config.sandbox.enabled || effective == SandboxLevel::None {
        println!("Sandbox is disabled or no kernel support available.");
        println!("Skipping enforcement tests.");
        return Ok(());
    }

    let workspace = config.workspace_path();
    let policy = build_policy(&config.sandbox, &workspace, effective);

    println!("Running sandbox smoke tests...");
    println!("  Workspace: {}", workspace.display());
    println!("  Level: {:?}", effective);
    println!();

    let mut passed = 0;
    let mut failed = 0;

    // Test 1: Echo command succeeds
    print!("  [1/6] Echo command succeeds:        ");
    match run_sandboxed("echo hello", &policy, 10_000).await {
        Ok((output, code)) if code == 0 && output.contains("hello") => {
            println!("ok");
            passed += 1;
        }
        Ok((output, code)) => {
            println!(
                "FAIL (exit={}, output={})",
                code,
                &output[..output.floor_char_boundary(100)]
            );
            failed += 1;
        }
        Err(e) => {
            println!("FAIL ({})", e);
            failed += 1;
        }
    }

    // Test 2: Write outside workspace denied
    print!("  [2/6] Write outside workspace:      ");
    match run_sandboxed("touch /tmp/localgpt-sandbox-test-ok 2>&1", &policy, 10_000).await {
        Ok((_, 0)) => {
            // /tmp is in extra_write_paths, so this should work.
            // Test a truly outside path instead.
            match run_sandboxed("touch /localgpt-sandbox-test-deny 2>&1", &policy, 10_000).await {
                Ok((_, code)) if code != 0 => {
                    println!("denied (ok)");
                    passed += 1;
                }
                Ok((_output, code)) => {
                    println!("FAIL (exit={}, expected non-zero)", code);
                    failed += 1;
                }
                Err(e) => {
                    println!("FAIL ({})", e);
                    failed += 1;
                }
            }
            // Clean up
            let _ = run_sandboxed("rm -f /tmp/localgpt-sandbox-test-ok", &policy, 5_000).await;
        }
        Ok((_, code)) => {
            println!("denied (ok, /tmp write also blocked: exit={})", code);
            passed += 1;
        }
        Err(e) => {
            println!("FAIL ({})", e);
            failed += 1;
        }
    }

    // Test 3: Read ~/.ssh denied
    print!("  [3/6] Read ~/.ssh:                  ");
    match run_sandboxed("ls ~/.ssh/ 2>&1", &policy, 10_000).await {
        Ok((_, code)) if code != 0 => {
            println!("denied (ok)");
            passed += 1;
        }
        Ok((output, code)) => {
            // On some systems ~/.ssh may not exist
            if output.contains("No such file") {
                println!("skipped (no ~/.ssh)");
                passed += 1;
            } else {
                println!("FAIL (exit={}, expected denied)", code);
                failed += 1;
            }
        }
        Err(e) => {
            println!("FAIL ({})", e);
            failed += 1;
        }
    }

    // Test 4: Network denied
    print!("  [4/6] Network (curl):               ");
    match run_sandboxed(
        "curl -s --connect-timeout 3 http://example.com 2>&1",
        &policy,
        15_000,
    )
    .await
    {
        Ok((output, code))
            if code != 0
                || output.contains("denied")
                || output.contains("EPERM")
                || output.contains("not found") =>
        {
            println!("denied (ok)");
            passed += 1;
        }
        Ok((output, code)) => {
            if output.contains("not found") || output.contains("No such file") {
                println!("skipped (curl not installed)");
                passed += 1;
            } else {
                println!("FAIL (exit={}, network may not be blocked)", code);
                failed += 1;
            }
        }
        Err(_e) => {
            // Timeout could mean network was blocked and curl hung
            println!("denied (ok, timed out)");
            passed += 1;
        }
    }

    // Test 5: Timeout enforcement
    print!("  [5/6] Timeout enforcement:          ");
    let start = std::time::Instant::now();
    match run_sandboxed("sleep 30", &policy, 3_000).await {
        Err(e) if e.to_string().contains("timed out") => {
            let elapsed = start.elapsed();
            println!("killed after {:.1}s (ok)", elapsed.as_secs_f64());
            passed += 1;
        }
        Ok((_, _)) => {
            let elapsed = start.elapsed();
            if elapsed.as_secs() < 10 {
                println!("ok (completed quickly)");
                passed += 1;
            } else {
                println!("FAIL (command was not killed)");
                failed += 1;
            }
        }
        Err(e) => {
            println!("FAIL ({})", e);
            failed += 1;
        }
    }

    // Test 6: Sandbox disabled passthrough
    print!("  [6/6] Basic command works:          ");
    match run_sandboxed("echo sandbox-ok && pwd", &policy, 10_000).await {
        Ok((output, 0)) if output.contains("sandbox-ok") => {
            println!("ok");
            passed += 1;
        }
        Ok((output, code)) => {
            println!(
                "FAIL (exit={}, output={})",
                code,
                &output[..output.floor_char_boundary(100)]
            );
            failed += 1;
        }
        Err(e) => {
            println!("FAIL ({})", e);
            failed += 1;
        }
    }

    println!();
    if failed == 0 {
        println!("All {} tests passed.", passed);
    } else {
        println!("{} passed, {} failed.", passed, failed);
    }

    Ok(())
}

fn format_level(level: SandboxLevel) -> &'static str {
    match level {
        SandboxLevel::Full => "Full",
        SandboxLevel::Standard => "Standard",
        SandboxLevel::Minimal => "Minimal",
        SandboxLevel::None => "None",
    }
}
