//! This mainly concerns converting collected data into things that the canvas
//! can actually handle.

use crate::app::data_farmer::DataCollection;
use crate::app::data_harvester::cpu::CpuDataType;
use crate::app::data_harvester::temperature::TemperatureType;
use crate::app::widgets::{DiskWidgetData, TempWidgetData};
use crate::components::old_text_table::CellContent;
use crate::components::time_graph::Point;
use crate::utils::gen_util::*;
use crate::{app::AxisScaling, units::data_units::DataUnit, Pid};

use fxhash::FxHashMap;
use kstring::KString;

#[derive(Default, Debug)]
pub struct ConvertedBatteryData {
    pub battery_name: String,
    pub charge_percentage: f64,
    pub watt_consumption: String,
    pub duration_until_full: Option<String>,
    pub duration_until_empty: Option<String>,
    pub health: String,
}

#[derive(Default, Debug)]
pub struct TableData {
    pub data: Vec<TableRow>,
    pub col_widths: Vec<usize>,
}

#[derive(Debug)]
pub enum TableRow {
    Raw(Vec<CellContent>),
    Styled(Vec<CellContent>, tui::style::Style),
}

impl TableRow {
    pub fn row(&self) -> &[CellContent] {
        match self {
            TableRow::Raw(data) => data,
            TableRow::Styled(data, _) => data,
        }
    }
}

#[derive(Default, Debug)]
pub struct ConvertedNetworkData {
    pub rx: Vec<Point>,
    pub tx: Vec<Point>,
    pub rx_display: String,
    pub tx_display: String,
    pub total_rx_display: Option<String>,
    pub total_tx_display: Option<String>,
    // TODO: [NETWORKING] add min/max/mean of each
    // min_rx : f64,
    // max_rx : f64,
    // mean_rx: f64,
    // min_tx: f64,
    // max_tx: f64,
    // mean_tx: f64,
}

#[derive(Clone, Debug)]
pub enum CpuWidgetData {
    All,
    Entry {
        data_type: CpuDataType,
        /// A point here represents time (x) and value (y).
        data: Vec<Point>,
        last_entry: f64,
    },
}

#[derive(Default)]
pub struct ConvertedData {
    pub rx_display: String,
    pub tx_display: String,
    pub total_rx_display: String,
    pub total_tx_display: String,
    pub network_data_rx: Vec<Point>,
    pub network_data_tx: Vec<Point>,
    pub disk_data: Vec<DiskWidgetData>,
    pub temp_data: Vec<TempWidgetData>,

    /// A mapping from a process name to any PID with that name.
    pub process_name_pid_map: FxHashMap<String, Vec<Pid>>,

    /// A mapping from a process command to any PID with that name.
    pub process_cmd_pid_map: FxHashMap<String, Vec<Pid>>,

    pub mem_labels: Option<(String, String)>,
    pub swap_labels: Option<(String, String)>,

    pub mem_data: Vec<Point>, // TODO: Switch this and all data points over to a better data structure...
    pub swap_data: Vec<Point>,
    pub load_avg_data: [f32; 3],
    pub cpu_data: Vec<CpuWidgetData>,
    pub battery_data: Vec<ConvertedBatteryData>,
}

impl ConvertedData {
    // TODO: Can probably heavily reduce this step to avoid clones.
    pub fn ingest_disk(&mut self, data: &DataCollection) {
        self.disk_data.clear();

        data.disk_harvest
            .iter()
            .zip(&data.io_labels)
            .for_each(|(disk, (io_read, io_write))| {
                self.disk_data.push(DiskWidgetData {
                    name: KString::from_ref(&disk.name),
                    mount_point: KString::from_ref(&disk.mount_point),
                    free_bytes: disk.free_space,
                    used_bytes: disk.used_space,
                    total_bytes: disk.total_space,
                    io_read: io_read.into(),
                    io_write: io_write.into(),
                });
            });

        self.disk_data.shrink_to_fit();
    }

    pub fn ingest_temp(&mut self, data: &DataCollection, temperature_type: TemperatureType) {
        self.temp_data.clear();

        data.temp_harvest.iter().for_each(|temp_harvest| {
            self.temp_data.push(TempWidgetData {
                sensor: KString::from_ref(&temp_harvest.name),
                temperature_value: temp_harvest.temperature.ceil() as u64,
                temperature_type,
            });
        });

        self.temp_data.shrink_to_fit();
    }

    pub fn convert_cpu_data_points(&mut self, current_data: &DataCollection) {
        let current_time = if let Some(frozen_instant) = current_data.frozen_instant {
            frozen_instant
        } else {
            current_data.current_instant
        };

        // (Re-)initialize the vector if the lengths don't match...
        if let Some((_time, data)) = &current_data.timed_data_vec.last() {
            if data.cpu_data.len() + 1 != self.cpu_data.len() {
                self.cpu_data = Vec::with_capacity(data.cpu_data.len() + 1);
                self.cpu_data.push(CpuWidgetData::All);
                self.cpu_data.extend(
                    data.cpu_data
                        .iter()
                        .zip(&current_data.cpu_harvest)
                        .map(|(cpu_usage, data)| CpuWidgetData::Entry {
                            data_type: data.data_type,
                            data: vec![],
                            last_entry: *cpu_usage,
                        })
                        .collect::<Vec<CpuWidgetData>>(),
                );
            } else {
                self.cpu_data
                    .iter_mut()
                    .skip(1)
                    .zip(&data.cpu_data)
                    .for_each(|(cpu, cpu_usage)| match cpu {
                        CpuWidgetData::All => unreachable!(),
                        CpuWidgetData::Entry {
                            data_type: _,
                            data,
                            last_entry,
                        } => {
                            // A bit faster to just update all the times, so we just clear the vector.
                            data.clear();
                            *last_entry = *cpu_usage;
                        }
                    });
            }
        }

        // Now push all the data.
        for (itx, cpu) in &mut self.cpu_data.iter_mut().skip(1).enumerate() {
            match cpu {
                CpuWidgetData::All => unreachable!(),
                CpuWidgetData::Entry {
                    data_type: _,
                    data,
                    last_entry: _,
                } => {
                    for (time, timed_data) in &current_data.timed_data_vec {
                        let time_start: f64 =
                            (current_time.duration_since(*time).as_millis() as f64).floor();

                        if let Some(val) = timed_data.cpu_data.get(itx) {
                            data.push((-time_start, *val));
                        }

                        if *time == current_time {
                            break;
                        }
                    }

                    data.shrink_to_fit();
                }
            }
        }
    }
}

pub fn convert_mem_data_points(current_data: &DataCollection) -> Vec<Point> {
    let mut result: Vec<Point> = Vec::new();
    let current_time = if let Some(frozen_instant) = current_data.frozen_instant {
        frozen_instant
    } else {
        current_data.current_instant
    };

    for (time, data) in &current_data.timed_data_vec {
        if let Some(mem_data) = data.mem_data {
            let time_from_start: f64 =
                (current_time.duration_since(*time).as_millis() as f64).floor();
            result.push((-time_from_start, mem_data));
            if *time == current_time {
                break;
            }
        }
    }

    result
}

pub fn convert_swap_data_points(current_data: &DataCollection) -> Vec<Point> {
    let mut result: Vec<Point> = Vec::new();
    let current_time = if let Some(frozen_instant) = current_data.frozen_instant {
        frozen_instant
    } else {
        current_data.current_instant
    };

    for (time, data) in &current_data.timed_data_vec {
        if let Some(swap_data) = data.swap_data {
            let time_from_start: f64 =
                (current_time.duration_since(*time).as_millis() as f64).floor();
            result.push((-time_from_start, swap_data));
            if *time == current_time {
                break;
            }
        }
    }

    result
}

pub fn convert_mem_labels(
    current_data: &DataCollection,
) -> (Option<(String, String)>, Option<(String, String)>) {
    /// Returns the unit type and denominator for given total amount of memory in kibibytes.
    fn return_unit_and_denominator_for_mem_kib(mem_total_kib: u64) -> (&'static str, f64) {
        if mem_total_kib < 1024 {
            // Stay with KiB
            ("KiB", 1.0)
        } else if mem_total_kib < MEBI_LIMIT {
            // Use MiB
            ("MiB", KIBI_LIMIT_F64)
        } else if mem_total_kib < GIBI_LIMIT {
            // Use GiB
            ("GiB", MEBI_LIMIT_F64)
        } else {
            // Use TiB
            ("TiB", GIBI_LIMIT_F64)
        }
    }

    (
        if current_data.memory_harvest.mem_total_in_kib > 0 {
            Some((
                format!(
                    "{:3.0}%",
                    current_data.memory_harvest.use_percent.unwrap_or(0.0)
                ),
                {
                    let (unit, denominator) = return_unit_and_denominator_for_mem_kib(
                        current_data.memory_harvest.mem_total_in_kib,
                    );

                    format!(
                        "   {:.1}{}/{:.1}{}",
                        current_data.memory_harvest.mem_used_in_kib as f64 / denominator,
                        unit,
                        (current_data.memory_harvest.mem_total_in_kib as f64 / denominator),
                        unit
                    )
                },
            ))
        } else {
            None
        },
        if current_data.swap_harvest.mem_total_in_kib > 0 {
            Some((
                format!(
                    "{:3.0}%",
                    current_data.swap_harvest.use_percent.unwrap_or(0.0)
                ),
                {
                    let (unit, denominator) = return_unit_and_denominator_for_mem_kib(
                        current_data.swap_harvest.mem_total_in_kib,
                    );

                    format!(
                        "   {:.1}{}/{:.1}{}",
                        current_data.swap_harvest.mem_used_in_kib as f64 / denominator,
                        unit,
                        (current_data.swap_harvest.mem_total_in_kib as f64 / denominator),
                        unit
                    )
                },
            ))
        } else {
            None
        },
    )
}

pub fn get_rx_tx_data_points(
    current_data: &DataCollection, network_scale_type: &AxisScaling, network_unit_type: &DataUnit,
    network_use_binary_prefix: bool,
) -> (Vec<Point>, Vec<Point>) {
    let mut rx: Vec<Point> = Vec::new();
    let mut tx: Vec<Point> = Vec::new();

    let current_time = if let Some(frozen_instant) = current_data.frozen_instant {
        frozen_instant
    } else {
        current_data.current_instant
    };

    for (time, data) in &current_data.timed_data_vec {
        let time_from_start: f64 = (current_time.duration_since(*time).as_millis() as f64).floor();

        let (rx_data, tx_data) = match network_scale_type {
            AxisScaling::Log => {
                if network_use_binary_prefix {
                    match network_unit_type {
                        DataUnit::Byte => {
                            // As dividing by 8 is equal to subtracting 4 in base 2!
                            ((data.rx_data).log2() - 4.0, (data.tx_data).log2() - 4.0)
                        }
                        DataUnit::Bit => ((data.rx_data).log2(), (data.tx_data).log2()),
                    }
                } else {
                    match network_unit_type {
                        DataUnit::Byte => {
                            ((data.rx_data / 8.0).log10(), (data.tx_data / 8.0).log10())
                        }
                        DataUnit::Bit => ((data.rx_data).log10(), (data.tx_data).log10()),
                    }
                }
            }
            AxisScaling::Linear => match network_unit_type {
                DataUnit::Byte => (data.rx_data / 8.0, data.tx_data / 8.0),
                DataUnit::Bit => (data.rx_data, data.tx_data),
            },
        };

        rx.push((-time_from_start, rx_data));
        tx.push((-time_from_start, tx_data));
        if *time == current_time {
            break;
        }
    }

    (rx, tx)
}

pub fn convert_network_data_points(
    current_data: &DataCollection, need_four_points: bool, network_scale_type: &AxisScaling,
    network_unit_type: &DataUnit, network_use_binary_prefix: bool,
) -> ConvertedNetworkData {
    let (rx, tx) = get_rx_tx_data_points(
        current_data,
        network_scale_type,
        network_unit_type,
        network_use_binary_prefix,
    );

    let unit = match network_unit_type {
        DataUnit::Byte => "B/s",
        DataUnit::Bit => "b/s",
    };

    let (rx_data, tx_data, total_rx_data, total_tx_data) = match network_unit_type {
        DataUnit::Byte => (
            current_data.network_harvest.rx / 8,
            current_data.network_harvest.tx / 8,
            current_data.network_harvest.total_rx / 8,
            current_data.network_harvest.total_tx / 8,
        ),
        DataUnit::Bit => (
            current_data.network_harvest.rx,
            current_data.network_harvest.tx,
            current_data.network_harvest.total_rx / 8, // We always make this bytes...
            current_data.network_harvest.total_tx / 8,
        ),
    };

    let (rx_converted_result, total_rx_converted_result): ((f64, String), (f64, String)) =
        if network_use_binary_prefix {
            (
                get_binary_prefix(rx_data, unit), // If this isn't obvious why there's two functions, one you can configure the unit, the other is always bytes
                get_binary_bytes(total_rx_data),
            )
        } else {
            (
                get_decimal_prefix(rx_data, unit),
                get_decimal_bytes(total_rx_data),
            )
        };

    let (tx_converted_result, total_tx_converted_result): ((f64, String), (f64, String)) =
        if network_use_binary_prefix {
            (
                get_binary_prefix(tx_data, unit),
                get_binary_bytes(total_tx_data),
            )
        } else {
            (
                get_decimal_prefix(tx_data, unit),
                get_decimal_bytes(total_tx_data),
            )
        };

    if need_four_points {
        let rx_display = format!("{:.*}{}", 1, rx_converted_result.0, rx_converted_result.1);
        let total_rx_display = Some(format!(
            "{:.*}{}",
            1, total_rx_converted_result.0, total_rx_converted_result.1
        ));
        let tx_display = format!("{:.*}{}", 1, tx_converted_result.0, tx_converted_result.1);
        let total_tx_display = Some(format!(
            "{:.*}{}",
            1, total_tx_converted_result.0, total_tx_converted_result.1
        ));
        ConvertedNetworkData {
            rx,
            tx,
            rx_display,
            tx_display,
            total_rx_display,
            total_tx_display,
        }
    } else {
        let rx_display = format!(
            "RX: {:<10}  All: {}",
            if network_use_binary_prefix {
                format!("{:.1}{:3}", rx_converted_result.0, rx_converted_result.1)
            } else {
                format!("{:.1}{:2}", rx_converted_result.0, rx_converted_result.1)
            },
            if network_use_binary_prefix {
                format!(
                    "{:.1}{:3}",
                    total_rx_converted_result.0, total_rx_converted_result.1
                )
            } else {
                format!(
                    "{:.1}{:2}",
                    total_rx_converted_result.0, total_rx_converted_result.1
                )
            }
        );
        let tx_display = format!(
            "TX: {:<10}  All: {}",
            if network_use_binary_prefix {
                format!("{:.1}{:3}", tx_converted_result.0, tx_converted_result.1)
            } else {
                format!("{:.1}{:2}", tx_converted_result.0, tx_converted_result.1)
            },
            if network_use_binary_prefix {
                format!(
                    "{:.1}{:3}",
                    total_tx_converted_result.0, total_tx_converted_result.1
                )
            } else {
                format!(
                    "{:.1}{:2}",
                    total_tx_converted_result.0, total_tx_converted_result.1
                )
            }
        );

        ConvertedNetworkData {
            rx,
            tx,
            rx_display,
            tx_display,
            total_rx_display: None,
            total_tx_display: None,
        }
    }
}

/// Returns a string given a value that is converted to the closest binary variant.
/// If the value is greater than a gibibyte, then it will return a decimal place.
pub fn binary_byte_string(value: u64) -> String {
    let converted_values = get_binary_bytes(value);
    if value >= GIBI_LIMIT {
        format!("{:.*}{}", 1, converted_values.0, converted_values.1)
    } else {
        format!("{:.*}{}", 0, converted_values.0, converted_values.1)
    }
}

/// Returns a string given a value that is converted to the closest SI-variant, per second.
/// If the value is greater than a giga-X, then it will return a decimal place.
pub fn dec_bytes_per_second_string(value: u64) -> String {
    let converted_values = get_decimal_bytes(value);
    if value >= GIGA_LIMIT {
        format!("{:.*}{}/s", 1, converted_values.0, converted_values.1)
    } else {
        format!("{:.*}{}/s", 0, converted_values.0, converted_values.1)
    }
}

#[cfg(feature = "battery")]
pub fn convert_battery_harvest(current_data: &DataCollection) -> Vec<ConvertedBatteryData> {
    current_data
        .battery_harvest
        .iter()
        .enumerate()
        .map(|(itx, battery_harvest)| ConvertedBatteryData {
            battery_name: format!("Battery {}", itx),
            charge_percentage: battery_harvest.charge_percent,
            watt_consumption: format!("{:.2}W", battery_harvest.power_consumption_rate_watts),
            duration_until_empty: if let Some(secs_till_empty) = battery_harvest.secs_until_empty {
                let time = time::Duration::seconds(secs_till_empty);
                let num_minutes = time.whole_minutes() - time.whole_hours() * 60;
                let num_seconds = time.whole_seconds() - time.whole_minutes() * 60;
                Some(format!(
                    "{} hour{}, {} minute{}, {} second{}",
                    time.whole_hours(),
                    if time.whole_hours() == 1 { "" } else { "s" },
                    num_minutes,
                    if num_minutes == 1 { "" } else { "s" },
                    num_seconds,
                    if num_seconds == 1 { "" } else { "s" },
                ))
            } else {
                None
            },
            duration_until_full: if let Some(secs_till_full) = battery_harvest.secs_until_full {
                let time = time::Duration::seconds(secs_till_full);
                let num_minutes = time.whole_minutes() - time.whole_hours() * 60;
                let num_seconds = time.whole_seconds() - time.whole_minutes() * 60;
                Some(format!(
                    "{} hour{}, {} minute{}, {} second{}",
                    time.whole_hours(),
                    if time.whole_hours() == 1 { "" } else { "s" },
                    num_minutes,
                    if num_minutes == 1 { "" } else { "s" },
                    num_seconds,
                    if num_seconds == 1 { "" } else { "s" },
                ))
            } else {
                None
            },
            health: format!("{:.2}%", battery_harvest.health_percent),
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_binary_byte_string() {
        assert_eq!(binary_byte_string(0), "0B".to_string());
        assert_eq!(binary_byte_string(1), "1B".to_string());
        assert_eq!(binary_byte_string(1000), "1000B".to_string());
        assert_eq!(binary_byte_string(1023), "1023B".to_string());
        assert_eq!(binary_byte_string(KIBI_LIMIT), "1KiB".to_string());
        assert_eq!(binary_byte_string(KIBI_LIMIT + 1), "1KiB".to_string());
        assert_eq!(binary_byte_string(MEBI_LIMIT), "1MiB".to_string());
        assert_eq!(binary_byte_string(GIBI_LIMIT), "1.0GiB".to_string());
        assert_eq!(binary_byte_string(2 * GIBI_LIMIT), "2.0GiB".to_string());
        assert_eq!(
            binary_byte_string((2.5 * GIBI_LIMIT as f64) as u64),
            "2.5GiB".to_string()
        );
        assert_eq!(
            binary_byte_string((10.34 * TEBI_LIMIT as f64) as u64),
            "10.3TiB".to_string()
        );
        assert_eq!(
            binary_byte_string((10.36 * TEBI_LIMIT as f64) as u64),
            "10.4TiB".to_string()
        );
    }

    #[test]
    fn test_dec_bytes_per_second_string() {
        assert_eq!(dec_bytes_per_second_string(0), "0B/s".to_string());
        assert_eq!(dec_bytes_per_second_string(1), "1B/s".to_string());
        assert_eq!(dec_bytes_per_second_string(900), "900B/s".to_string());
        assert_eq!(dec_bytes_per_second_string(999), "999B/s".to_string());
        assert_eq!(dec_bytes_per_second_string(KILO_LIMIT), "1KB/s".to_string());
        assert_eq!(
            dec_bytes_per_second_string(KILO_LIMIT + 1),
            "1KB/s".to_string()
        );
        assert_eq!(dec_bytes_per_second_string(KIBI_LIMIT), "1KB/s".to_string());
        assert_eq!(dec_bytes_per_second_string(MEGA_LIMIT), "1MB/s".to_string());
        assert_eq!(
            dec_bytes_per_second_string(GIGA_LIMIT),
            "1.0GB/s".to_string()
        );
        assert_eq!(
            dec_bytes_per_second_string(2 * GIGA_LIMIT),
            "2.0GB/s".to_string()
        );
        assert_eq!(
            dec_bytes_per_second_string((2.5 * GIGA_LIMIT as f64) as u64),
            "2.5GB/s".to_string()
        );
        assert_eq!(
            dec_bytes_per_second_string((10.34 * TERA_LIMIT as f64) as u64),
            "10.3TB/s".to_string()
        );
        assert_eq!(
            dec_bytes_per_second_string((10.36 * TERA_LIMIT as f64) as u64),
            "10.4TB/s".to_string()
        );
    }
}
