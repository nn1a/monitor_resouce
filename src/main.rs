use std::io;
use std::process::exit;
use std::{env, fs, thread, time::Duration};

fn read_stat(pid: i32) -> Result<Vec<String>, String> {
    let stat_path = format!("/proc/{}/stat", pid);
    let stat_content = fs::read_to_string(stat_path)
        .map_err(|err| format!("Failed to access process with PID {}: {}", pid, err))?;
    Ok(stat_content.split_whitespace().map(String::from).collect())
}

fn get_memory_usage(fields: &[String]) -> u64 {
    if let Some(rss_pages) = fields.get(23).and_then(|s| s.parse::<u64>().ok()) {
        rss_pages * 4 // Convert pages to KB (assuming 4KB page size)
    } else {
        0
    }
}

// fn read_cmdline(pid: i32) -> String {
//     let cmdline_path = format!("/proc/{}/cmdline", pid);
//     fs::read_to_string(&cmdline_path)
//         .map(|content| content.replace("\0", " "))
//         .unwrap_or_else(|_| String::from("[Failed to read cmdline]"))
// }

fn get_all_children_pids(parent_pid: i32) -> io::Result<Vec<i32>> {
    fn read_children_pids(pid: i32) -> io::Result<Vec<i32>> {
        let mut children_pids_vec = Vec::new();
        let task_path = format!("/proc/{}/task/{}/children", pid, pid);
        if let Ok(children_pids) = fs::read_to_string(task_path) {
            for child_pid_str in children_pids.split_whitespace() {
                if let Ok(child_pid) = child_pid_str.parse::<i32>() {
                    children_pids_vec.push(child_pid);
                }
            }
        }
        Ok(children_pids_vec)
    }

    fn get_all_children_recursive(parent_pid: i32) -> io::Result<Vec<i32>> {
        let mut all_children = vec![parent_pid];
        let mut queue = vec![parent_pid];

        while let Some(pid) = queue.pop() {
            let children_pids = read_children_pids(pid)?;
            for child_pid in children_pids {
                all_children.push(child_pid);
                queue.push(child_pid);
            }
        }

        Ok(all_children)
    }

    get_all_children_recursive(parent_pid)
}

fn get_mem_usage(pid: i32) -> u64 {
    let fields = match read_stat(pid) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{}", e);
            return 0;
        }
    };

    get_memory_usage(&fields)
}

fn get_mem_usage_pids(pids: &[i32]) -> u64 {
    let mut mem = 0;
    for pid in pids {
        mem += get_mem_usage(*pid);
    }
    mem
}

fn get_cpu_usage(fields: &[String]) -> u64 {
    let utime = fields
        .get(13)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let stime = fields
        .get(14)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    utime + stime
}

fn get_system_cpu_usage() -> Result<u64, String> {
    let stat_content = fs::read_to_string("/proc/stat")
        .map_err(|err| format!("Failed to access /proc/stat: {}", err))?;
    let first_line = stat_content
        .lines()
        .next()
        .ok_or("Failed to read first line of /proc/stat".to_string())?;
    let total_cpu_time: u64 = first_line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse::<u64>().ok())
        .sum();
    Ok(total_cpu_time)
}

fn get_cpu_tick(pids: &[i32]) -> Vec<u64> {
    let mut data = Vec::new();
    for child_pid in pids {
        let fields_before = read_stat(*child_pid).unwrap();
        let cpu_usage_before = get_cpu_usage(&fields_before);
        data.push(cpu_usage_before);
    }
    data
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <parent_pid> <interval>", args[0]);
        exit(1);
    }

    let parent_pid: i32 = args[1].parse().expect("Invalid PID");
    let interval: u64 = args[2].parse().expect("Invalid interval");

    loop {
        let pids = get_all_children_pids(parent_pid).unwrap();

        let system_cpu_usage_before = get_system_cpu_usage().unwrap();
        let before_cpu = get_cpu_tick(pids.as_slice());

        let mem: u64 = get_mem_usage_pids(pids.as_slice());

        thread::sleep(Duration::from_secs(interval));

        let after_cpu = get_cpu_tick(pids.as_slice());
        let system_cpu_usage_after = get_system_cpu_usage().unwrap();
        let delta_sys = system_cpu_usage_after.saturating_sub(system_cpu_usage_before);

        let mut delta: f64 = 0.0;
        for (before_tick, after_tick) in before_cpu.iter().zip(after_cpu.iter()) {
            let delta_proc = after_tick.saturating_sub(*before_tick);
            if delta_proc == 0 {
                continue;
            }
            delta += (delta_proc as f64 / delta_sys as f64) * 100.0;
        }
        println!(
            "Total PID: {},  Memory: {} KB CPU {:.2}",
            parent_pid, mem, delta
        );
    }
}
