use std::{
    sync::{Arc, Mutex, atomic::AtomicI64},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use tokio::io::AsyncWriteExt;

#[cfg(target_family = "unix")]
unsafe fn libc_sysconf(name: i32) -> i64 {
    unsafe extern "C" {
        fn sysconf(name: i32) -> i64;
    }
    unsafe { sysconf(name) }
}

#[cfg(target_family = "unix")]
const _SC_PAGESIZE: i32 = 30; // usually 30 on Linux

// Values from Linux headers
#[cfg(target_os = "linux")]
const _SC_CLK_TCK: i32 = 2;

pub async fn write_metrics(
    output_path: String,
    interval: Duration,
    active_queries: Arc<AtomicI64>,
    failed_queries: Arc<AtomicI64>,
    query_times: Arc<Mutex<Vec<u64>>>,
) -> anyhow::Result<()> {
    let process_stat_file_path = "/proc/self/stat";
    let mut f = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(output_path)
        .await
        .unwrap();

    f.write_all(b"timestamp_ms,active_number_of_queries,failed_number_of_queries,number_of_queries_processed,average_query_time_us,cpu_milliseconds,memory_bytes\n")
    .await
    .unwrap();

    let mut query_metrics_file = f;

    let page_size = unsafe { libc_sysconf(_SC_PAGESIZE) } as u64;
    let cpu_jiffies_per_second = unsafe { libc_sysconf(_SC_CLK_TCK) } as u64;
    println!(
        "PAGE_SIZE: {}, CPU_JIFFIES_PER_SECOND: {}",
        page_size, cpu_jiffies_per_second
    );

    // let (page_size, cpu_jiffies_per_second, mut query_metrics_file) = futures::future::join3(f1, f2, f3).await;

    let mut last_cpu_jiffies = 0u64;

    let mut write_buffer = String::with_capacity(1024);

    let mut tick_interval = tokio::time::interval(interval);
    tick_interval.tick().await;
    loop {
        tick_interval.tick().await;
        let now = SystemTime::now();
        let now_ms = now.duration_since(UNIX_EPOCH)?.as_millis();

        let active_number_of_queries = active_queries.load(std::sync::atomic::Ordering::Relaxed);
        let failed_number_of_queries = failed_queries.swap(0, std::sync::atomic::Ordering::Relaxed);

        let (sum, len) = {
            let mut query_times = query_times.lock().unwrap();
            let len = query_times.len();

            if len == 0 && active_number_of_queries == 0 {
                println!("0 Queries processed and 0 Active Queries");
                break;
            }

            let mut sum = 0u64;
            for q in query_times.iter() {
                sum += q;
            }

            query_times.clear();

            (sum, len)
        };

        let average = sum as f64 / len as f64;

        let stats = tokio::fs::read_to_string(process_stat_file_path).await?;

        let mut iter = stats.split_whitespace().skip(13);
        let cpu_user_jiffies = iter.next().context("not enough terms")?.parse::<u64>()?;
        let cpu_sys_jiffies = iter.next().context("not enough terms")?.parse::<u64>()?;
        let memory_pages_count = iter
            .skip(8)
            .next()
            .context("not enough terms")?
            .parse::<u64>()?;

        let total_cpu_jiffies = cpu_sys_jiffies + cpu_user_jiffies;

        let cpu_jiffies_in_interval = total_cpu_jiffies - last_cpu_jiffies;

        last_cpu_jiffies = total_cpu_jiffies;

        let total_memory_in_bytes = memory_pages_count * page_size;
        let cpu_time_spent_ms =
            (cpu_jiffies_in_interval * 1000) as f64 / cpu_jiffies_per_second as f64;

        write_buffer.clear();

        use std::fmt::Write;

        write!(
            write_buffer,
            "{},{},{},{},{:.2},{:.2},{}\n",
            now_ms,
            active_number_of_queries,
            failed_number_of_queries,
            len,
            average,
            cpu_time_spent_ms,
            total_memory_in_bytes,
        )?;

        print!("{}", write_buffer);

        query_metrics_file.write(write_buffer.as_bytes()).await?;
    }

    let cpu_time_spent_ms = (last_cpu_jiffies * 1000) as f64 / cpu_jiffies_per_second as f64;

    println!("Total CPU Time: {:.2}", cpu_time_spent_ms);

    Ok(())
}
