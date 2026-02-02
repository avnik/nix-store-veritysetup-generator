use std::env;
use std::fmt;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

const SYSTEMD_VERITYSETUP_PATH: &str = std::env!("SYSTEMD_VERITYSETUP_PATH");
const SYSTEMD_ESCAPE_PATH: &str = std::env!("SYSTEMD_ESCAPE_PATH");
const LUKS_VOLUME_GROUP: &str = "pool"; // FIXME: replace with std::env!("LUKS_VOLUME_GROUP") at final phase
const NIX_STORE: &str = "nix-store"; // FIXME: replace with std::env!("NIX_STORE") at final phase

/// The name of the service to create
const SERVICE_NAME: &str = "systemd-veritysetup@nix-store.service";

/// The name of the kernel commandline argument
const CMDLINE_ARG_NAME: &str = "storehash";
const GHAF_REVISION_NAME: &str = "ghaf.revision";

#[derive(Debug)]
struct Storehash {
    hash: String,
    revision: String,
}

impl Storehash {
    fn find_arg<'a>(cmdline: &'a str, key: &str) -> Option<&'a str> {
        cmdline
            .split_whitespace()
            .filter_map(|s| s.split_once('='))
            .find_map(|(k, v)| (k == key).then_some(v))
    }

    /// Parse the storehash from a provided kernel commandline
    fn from_cmdline(cmdline: &str) -> Option<Self> {
        let hash = Self::find_arg(cmdline, CMDLINE_ARG_NAME);
        if let Some(hash) = hash {
            if !hash.chars().all(|c| c.is_ascii_hexdigit()) || hash.len() != 64 {
                log::error!("ghaf-store-veritysetup-generator: {hash} is not in valid verity hash format. ignoring");
                return None;
            }
        }
        Some(Self {
            hash: hash?.into(),
            revision: Self::find_arg(cmdline, GHAF_REVISION_NAME)?.into(),
        })
    }

    fn hash_fragment(&self) -> &str {
        &self.hash[..16]
    }

    fn volume(&self, part: &str) -> Result<String> {
        let fragment = self.hash_fragment();
        Ok(format!(
            "/dev/mapper/{LUKS_VOLUME_GROUP}-{part}_{rev}_{fragment}",
            rev = self.revision
        ))
    }

    fn datadevice(&self) -> Result<String> {
        self.volume("root")
    }

    fn hashdevice(&self) -> Result<String> {
        self.volume("verity")
    }
}

impl fmt::Display for Storehash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // .revision is out from formatting intentionally, format used to render into systemd unit
        self.hash.fmt(f)
    }
}

/// Escape a string with `systemd-escape`.
fn systemd_escape(s: &str) -> Result<String> {
    let mut output = Command::new(SYSTEMD_ESCAPE_PATH)
        .arg(s)
        .output()
        .with_context(|| format!("Failed to run systemd-escape: {SYSTEMD_ESCAPE_PATH}"))?;
    if !output.status.success() {
        return Err(anyhow!("systemd-escape failed"));
    }
    // Remove newline from output
    output.stdout.pop();

    String::from_utf8(output.stdout)
        .context("Failed to convert systemd-escape output to a UTF-8 string")
}

/// Convert a path to a device into a systemd unit name.
///
/// For example: `/dev/vda` -> `dev-vda`
fn convert_to_unit(device_path: &str) -> Result<String> {
    let stripped = device_path
        .strip_prefix('/')
        .with_context(|| format!("Failed to strip '/' from {device_path}"))?;
    Ok(format!(
        "{}.device",
        systemd_escape(stripped).with_context(|| format!("Failed to escape {stripped}"))?
    ))
}

fn create_service_file(storehash: &Storehash) -> Result<String> {
    let datadevice = storehash.datadevice()?;
    let hashdevice = storehash.hashdevice()?;

    let datadevice_unit = convert_to_unit(&datadevice)
        .with_context(|| format!("Failed to convert {datadevice} to systemd unit name."))?;
    let hashdevice_unit = convert_to_unit(&hashdevice)
        .with_context(|| format!("Failed to convert {hashdevice} to systemd unit name."))?;

    let mut buffer = String::new();

    writeln!(
        &mut buffer,
        r#"[Unit]
Description=Integrity Protection Setup for %I
DefaultDependencies=no
IgnoreOnIsolate=true
After=veritysetup-pre.target systemd-udevd-kernel.socket
Before=blockdev@dev-mapper-%i.target
Wants=blockdev@dev-mapper-%i.target
Before=veritysetup.target
BindsTo={datadevice_unit} {hashdevice_unit}
After={datadevice_unit} {hashdevice_unit}"#
    )?;

    writeln!(
        &mut buffer,
        r#"[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart={SYSTEMD_VERITYSETUP_PATH} attach {NIX_STORE} {datadevice} {hashdevice} {storehash}
ExecStop={SYSTEMD_VERITYSETUP_PATH} detach {NIX_STORE}"#
    )?;

    Ok(buffer)
}

fn generator_symlink(
    destination_dir: &str,
    target_unit: &str,
    dependency_type: &str,
    src_unit: &str,
) -> Result<()> {
    let dir = format!("{destination_dir}/{target_unit}.{dependency_type}");
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {dir}"))?;

    let symlink = format!("{dir}/{src_unit}");
    let link_target = format!("../{src_unit}");
    std::os::unix::fs::symlink(link_target, &symlink)
        .with_context(|| format!("Failed to create symlink {symlink}"))?;

    Ok(())
}

fn generate() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let destination_dir = &args
        .get(1)
        .context("No command line argument is provided")?;

    let cmdline = fs::read_to_string("/proc/cmdline").context("Failed to read /proc/cmdline")?;
    let maybe_storehash = Storehash::from_cmdline(&cmdline);

    let storehash = match maybe_storehash {
        Some(s) => s,
        // If there is no storehash parameter on the cmdline just do nothing.
        None => {
            log::info!("ghaf-nix-store-veritysetup-generator: no valid parameters in /proc/cmdline, nothing to do");
            return Ok(());
        }
    };

    log::info!(
        "Using verity data device {}, hash device {}, and hash {} for {NIX_STORE}",
        storehash.datadevice()?,
        storehash.hashdevice()?,
        storehash,
    );

    let service_file = create_service_file(&storehash)?;

    // Write service to destination directory
    let service_file_path = format!("{destination_dir}/{SERVICE_NAME}");
    fs::write(&service_file_path, service_file.as_bytes())
        .with_context(|| format!("Failed to create {service_file_path}"))?;

    // Add a symlink to the destination directory so that the generated unit is pulled into the transaction
    generator_symlink(
        destination_dir,
        "veritysetup.target",
        "requires",
        SERVICE_NAME,
    )?;

    Ok(())
}

fn main() {
    kernlog::init().expect("Failed to initialize kernel logger");

    if let Err(e) = generate() {
        log::error!("{e:#}");
        std::process::exit(1);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    use expect_test::expect;

    #[test]
    fn parse_storehash_from_cmdline() {
        let expected_storehash = "94821122dbec8355df07f3670177b0cb147683a355c07da6a2fb85313cc02254";
        let expected_revision = "25.12.2";
        let cmdline = format!(
            "{CMDLINE_ARG_NAME}={expected_storehash} {GHAF_REVISION_NAME}={expected_revision}"
        );
        let storehash = Storehash::from_cmdline(&cmdline).unwrap();
        assert_eq!(storehash.hash, expected_storehash);
        assert_eq!(storehash.revision, expected_revision);
    }

    #[test]
    fn invalid_verity_hash_chars() {
        let expected_storehash = "invalid2dbec8355df07f3670177b0cb147683a355c07da6a2fb85313cc02254";
        let expected_revision = "25.12.2";
        let cmdline = format!(
            "{CMDLINE_ARG_NAME}={expected_storehash} {GHAF_REVISION_NAME}={expected_revision}"
        );
        assert!(Storehash::from_cmdline(&cmdline).is_none())
    }

    #[test]
    // Most important test, cutting 16 chars of too short hash could panic
    fn invalid_verity_hash_too_short() {
        let expected_storehash = "94821122db";
        let expected_revision = "25.12.2";
        let cmdline = format!(
            "{CMDLINE_ARG_NAME}={expected_storehash} {GHAF_REVISION_NAME}={expected_revision}"
        );
        assert!(Storehash::from_cmdline(&cmdline).is_none())
    }

    #[test]
    fn missing_hash() {
        let expected_revision = "25.12.2";
        let cmdline = format!("{GHAF_REVISION_NAME}={expected_revision}");
        assert!(Storehash::from_cmdline(&cmdline).is_none())
    }

    #[test]
    fn missing_revision() {
        let expected_storehash = "94821122dbec8355df07f3670177b0cb147683a355c07da6a2fb85313cc02254";
        let cmdline = format!("{CMDLINE_ARG_NAME}={expected_storehash}");
        assert!(Storehash::from_cmdline(&cmdline).is_none())
    }

    #[test]
    fn write_service_unit() {
        let storehash = Storehash::from_cmdline(&format!(
            "{CMDLINE_ARG_NAME}=94821122dbec8355df07f3670177b0cb147683a355c07da6a2fb85313cc02254 {GHAF_REVISION_NAME}=25.12.2"
        ))
        .unwrap();
        let actual_service_file = create_service_file(&storehash).unwrap();

        let expected_service_file = expect![[r#"
            [Unit]
            Description=Integrity Protection Setup for %I
            DefaultDependencies=no
            IgnoreOnIsolate=true
            After=veritysetup-pre.target systemd-udevd-kernel.socket
            Before=blockdev@dev-mapper-%i.target
            Wants=blockdev@dev-mapper-%i.target
            Before=veritysetup.target
            BindsTo=dev-mapper-pool\x2droot_25.12.2_94821122dbec8355.device dev-mapper-pool\x2dverity_25.12.2_94821122dbec8355.device
            After=dev-mapper-pool\x2droot_25.12.2_94821122dbec8355.device dev-mapper-pool\x2dverity_25.12.2_94821122dbec8355.device
            [Service]
            Type=oneshot
            RemainAfterExit=yes
            ExecStart=systemd-veritysetup attach nix-store /dev/mapper/pool-root_25.12.2_94821122dbec8355 /dev/mapper/pool-verity_25.12.2_94821122dbec8355 94821122dbec8355df07f3670177b0cb147683a355c07da6a2fb85313cc02254
            ExecStop=systemd-veritysetup detach nix-store
        "#]];

        expected_service_file.assert_eq(&actual_service_file);
    }
}
