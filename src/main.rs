use battery::units::ratio::part_per_hundred;
use battery::Battery;
use chrono::Local;
use std::io::{prelude::*, BufReader};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc::Sender;

static mut UPDATES: usize = 0;
static mut WORKSPACES: String = String::new();
static WHITE: &str = "#ffffff";
static GRAY: &str = "#888888";

async fn set_updates() {
    loop {
        let updates = String::from_utf8(
            Command::new("checkupdates")
                .stdout(Stdio::piped())
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let update_count = updates.trim().split("\n").collect::<Vec<&str>>().len();
        unsafe { UPDATES = update_count };
        tokio::time::sleep(Duration::new(600, 0)).await;
    }
}

fn set_workspaces(report: &String) {
    unsafe { WORKSPACES.clear() };
    let workspaces: Vec<&str> = report.split(":").collect();
    for workspace in workspaces {
        match workspace.chars().nth(0) {
            Some('O') => unsafe {
                WORKSPACES.push_str(&format!(
                    "%{{F{WHITE}}}{} ",
                    workspace.chars().last().unwrap()
                ))
            },
            Some('o') => unsafe {
                WORKSPACES.push_str(&format!(
                    "%{{F{GRAY}}}{} ",
                    workspace.chars().last().unwrap()
                ))
            },
            _ => {}
        };
    }
}

fn get_time() -> String {
    Local::now().format("%a %b %e, %T").to_string()
}

async fn clock(tx: Sender<()>) {
    loop {
        tokio::time::sleep(Duration::new(1, 0)).await;
        tx.send(()).await.unwrap();
    }
}

async fn bspc_subscribe(tx: Sender<()>) -> Result<(), std::io::Error> {
    let bspc = tokio::process::Command::new("bspc")
        .arg("subscribe")
        .arg("report")
        .stdout(Stdio::piped())
        .spawn()?
        .stdout
        .unwrap();

    let mut reader = tokio::io::BufReader::new(bspc);
    let mut buffer = String::new();
    loop {
        buffer.clear();
        reader.read_line(&mut buffer).await?;
        buffer = buffer.trim().to_string();
        // println!("{buffer}");
        set_workspaces(&buffer);
        tx.send(()).await.unwrap();
    }
}

async fn lemonbar_cmd(stdout: ChildStdout) {
    let reader = BufReader::new(stdout);

    reader
        .lines()
        .filter_map(|line| line.ok())
        .for_each(|line| {
            let mut args: Vec<&str> = line.split(" ").collect();
            Command::new(args.first().unwrap())
                .args(args.drain(1..))
                .spawn()
                .unwrap();
        });
}

fn get_battery(battery: &mut Option<Result<Battery, battery::Error>>) -> String {
    let mut charge: u8 = 0;
    let mut icon: &str = "";
    let icons;
    match battery {
        Some(Ok(battery)) => {
            battery.refresh().unwrap();
            charge = battery.state_of_charge().get::<part_per_hundred>() as u8;
            match battery.state() {
                battery::State::Discharging => {
                    icons = "%{F#cc0000}%{F-}    ";
                }
                battery::State::Charging => {
                    icons = "    ";
                }
                _ => icons = "",
            }
            if !icons.is_empty() {
                icon = icons.split(" ").nth(((charge - 1) / 20) as usize).unwrap();
            }
        }
        Some(Err(error)) => {
            println!("{:?}", error);
        }
        None => {}
    }
    format!("{}{charge}%", icon)
}

fn get_volume() -> String {
    let mute: String = String::from_utf8(
        Command::new("pactl")
            .args(["get-sink-mute", "0"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    if mute == "Mute: yes".to_string() {
        return "0%".to_string();
    }

    let volume: String = String::from_utf8(
        Command::new("pactl")
            .args(["get-sink-volume", "0"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .split("/")
    .nth(1)
    .unwrap()
    .trim()
    .to_owned();

    volume
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let mut battery = battery::Manager::new().unwrap().batteries().unwrap().next();
    let mut lemonbar = Command::new("lemonbar")
        .args([
            "-o",
            "-2",
            "-f",
            "mmcedar",
            "-f",
            "iosevka",
            "-g",
            "1888x20+16+6",
        ])
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()?;

    let mut lemonbar_stdin = lemonbar.stdin.take().unwrap();
    let lemonbar_stdout = lemonbar.stdout.take().unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(clock(tx.clone()));
    tokio::spawn(bspc_subscribe(tx.clone()));
    tokio::spawn(lemonbar_cmd(lemonbar_stdout));
    tokio::spawn(set_updates());

    while let Some(()) = rx.recv().await {
        update_bar(&lemonbar_stdin, &mut battery);
    }

    lemonbar_stdin.flush()?;
    drop(lemonbar_stdin);
    lemonbar.wait()?;

    Ok(())
}

fn update_bar(
    mut lemonbar_stdin: &ChildStdin,
    battery: &mut Option<Result<Battery, battery::Error>>,
) {
    let updates;
    unsafe { updates = UPDATES };
    let workspaces;
    unsafe { workspaces = &WORKSPACES };
    let volume = get_volume();
    let volume_str;
    if volume == "0%".to_string() {
        volume_str = "".to_string();
    } else {
        volume_str = format!("{volume}");
    }
    let mut updates_str = format!("");
    if updates > 1 {
        updates_str = format!("%{{A:kitty -e paru:}}{updates}%{{A}}");
    }
    write!(
        lemonbar_stdin,
        "%{{F{WHITE}}}%{{T2}}{}  {volume_str}  {updates_str}  %{{T1}}%{{c}}{workspaces} %{{F{WHITE}}}%{{T2}}%{{r}}{}",
        get_battery(battery),
        get_time()
    )
    .unwrap();
}
