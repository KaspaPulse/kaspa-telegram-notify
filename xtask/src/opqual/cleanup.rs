use super::*;
use anyhow::{Result, ensure};
use serde_json::json;
use std::{fs, time::Duration};

pub(super) fn execute(ctx: &ContextState) -> Result<()> {
    ctx.record(
        "CLEANUP",
        "PLANNED",
        "remove only task-labelled resources; preserve unrelated Docker state",
    )?;
    let before = unrelated_inventory()?;
    atomic_json(
        &ctx.run_dir.join("unrelated-resources-before-cleanup.json"),
        &before,
    )?;
    let mut errors = Vec::new();
    for (name, tries, fallback) in [
        (WEBHOOK_CONTAINER, 120, None),
        (APP_CONTAINER, 120, None),
        (LOCK_CONTAINER, 40, None),
        (TELEGRAM_CONTAINER, 40, Some("INT")),
        (HTTP_CONTAINER, 40, Some("INT")),
        (NODE_CONTAINER, 40, Some("INT")),
        (POSTGRES_CONTAINER, 120, None),
    ] {
        if let Err(e) = stop_owned(name, tries, fallback) {
            errors.push(format!("{name}: {e:#}"));
        }
    }
    if volume_exists(POSTGRES_VOLUME)? {
        assert_owned_volume(POSTGRES_VOLUME)?;
        if !run_status("sudo", ["-n", "docker", "volume", "rm", POSTGRES_VOLUME])?.0 {
            errors.push(format!("volume remove failed: {POSTGRES_VOLUME}"));
        }
    }
    if network_exists(DOCKER_NETWORK)? {
        assert_owned_network(DOCKER_NETWORK)?;
        if !run_status("sudo", ["-n", "docker", "network", "rm", DOCKER_NETWORK])?.0 {
            errors.push(format!("network remove failed: {DOCKER_NETWORK}"));
        }
    }
    for name in ["ca.key", "server.key", "server.csr", "ca.srl"] {
        let _ = fs::remove_file(ctx.certs_dir.join(name));
    }
    let containers = labeled_count("container")?;
    let networks = labeled_count("network")?;
    let volumes = labeled_count("volume")?;
    let after = unrelated_inventory()?;
    let preserved = before == after;
    let ok = errors.is_empty() && containers == 0 && networks == 0 && volumes == 0 && preserved;
    atomic_json(
        &ctx.run_dir.join("cleanup-receipt.json"),
        &json!({
            "task_containers":containers,"task_networks":networks,"task_volumes":volumes,
            "unrelated_baseline_resources_preserved":preserved,
            "forced_kill_used_for_application":false,"cleanup_ok":ok,"errors":errors
        }),
    )?;
    if ok {
        ctx.state_set("cleanup.completed", "true")?;
        ctx.mark_phase("cleanup", "VERIFIED")?;
        ctx.record(
            "CLEANUP",
            "VERIFIED",
            "task resources removed; unrelated baseline resources preserved",
        )?;
        Ok(())
    } else {
        ctx.mark_phase("cleanup", "FAILED")?;
        ctx.record(
            "CLEANUP",
            "FAILED",
            "cleanup incomplete without destructive force",
        )?;
        anyhow::bail!("cleanup incomplete; evidence preserved")
    }
}

fn stop_owned(name: &str, tries: usize, fallback: Option<&str>) -> Result<()> {
    if !container_exists(name)? {
        return Ok(());
    }
    assert_owned_container(name)?;
    if docker_inspect(name, "{{.State.Running}}")? == "true" {
        let _ = run_status("sudo", ["-n", "docker", "kill", "--signal=TERM", name])?;
        for _ in 0..tries {
            if docker_inspect(name, "{{.State.Running}}")? == "false" {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if docker_inspect(name, "{{.State.Running}}")? == "true"
            && let Some(signal) = fallback
        {
            let _ = run_status(
                "sudo",
                ["-n", "docker", "kill", &format!("--signal={signal}"), name],
            )?;
            for _ in 0..40 {
                if docker_inspect(name, "{{.State.Running}}")? == "false" {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        ensure!(
            docker_inspect(name, "{{.State.Running}}")? == "false",
            "{name} still running after graceful cleanup signals"
        );
    }
    run("sudo", ["-n", "docker", "rm", name])
}

fn labeled_count(kind: &str) -> Result<u64> {
    let filter = format!("label=task={TASK_ID}");
    let args: Vec<String> = match kind {
        "container" => [
            "-n",
            "docker",
            "ps",
            "-a",
            "--filter",
            filter.as_str(),
            "-q",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        "network" => [
            "-n",
            "docker",
            "network",
            "ls",
            "--filter",
            filter.as_str(),
            "-q",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        "volume" => [
            "-n",
            "docker",
            "volume",
            "ls",
            "--filter",
            filter.as_str(),
            "-q",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        _ => anyhow::bail!("unknown Docker kind"),
    };
    Ok(run_capture("sudo", args)?
        .lines()
        .filter(|x| !x.trim().is_empty())
        .count() as u64)
}
fn unrelated_inventory() -> Result<Value> {
    let containers = run_capture(
        "sudo",
        [
            "-n",
            "docker",
            "ps",
            "-a",
            "--format",
            r#"{{.ID}}|{{.Names}}|{{.Label "task"}}"#,
        ],
    )?;
    let networks = run_capture(
        "sudo",
        [
            "-n",
            "docker",
            "network",
            "ls",
            "--format",
            r#"{{.ID}}|{{.Name}}|{{.Label "task"}}"#,
        ],
    )?;
    let keep = |raw: String| {
        raw.lines()
            .filter_map(|line| {
                let mut p = line.split('|');
                let id = p.next()?;
                let name = p.next()?;
                let task = p.next().unwrap_or("");
                (task != TASK_ID).then(|| json!([id, name]))
            })
            .collect::<Vec<_>>()
    };
    Ok(json!({"containers":keep(containers),"networks":keep(networks)}))
}
