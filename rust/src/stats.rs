use strum::EnumString;
use strum_macros::{Display, EnumString, EnumVariantNames}; // Make sure to import EnumString

enum MeasurementKind {
    FileNameOnly,        // very fast, only reads the file name from trees
    FilePathOnly,        // not imlpemented, should be ok fast
    FileContents,        // very fast, reads the file contents from the ODB
    FileNameAndContents, // also fast, we preprocess the trees so that the filename is not an impediment
    FilePathAndContents, //not implemented. Should be somewhat fast.
    WholeRepo,           //not implemented. Will be slow
}
fn builtin_stat_map() -> HashMap<String, Stat> {
    static BUILTINS: OnceLock<HashMap<u32, &str>> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        //todo these should be exported by each stat module
        [
            (
                "tokei",
                Stat {
                    measurement_kind: MeasurementKind::FileNameAndContents,
                    blob_measurer: Box::new(FileContentsMeasurer {
                        callback: Box::new(TokeiCollector::new()),
                    }),
                    reducer: Box::new(NumMatchesReducer {}),
                },
            ),
            (
                "grep",
                Stat {
                    measurement_kind: MeasurementKind::FileContents,
                    blob_measurer: Box::new(FileContentsMeasurer {
                        callback: Box::new(RipgrepCollector::new(pattern)),
                    }),
                    reducer: Box::new(NumMatchesReducer {}),
                },
            ),
            (
                "filecount",
                Stat {
                    measurement_kind: MeasurementKind::FilePath,
                    blob_measurer: Box::new(FilePathMeasurer {
                        callback: Box::new(PathBlobCollector::new(pattern)),
                    }),
                },
            ),
            (
                "script",
                Stat {
                    measurement_kind: MeasurementKind::CustomScript,
                    blob_measurer: Box::new(CustomScriptMeasurer {
                        callback: Box::new(CustomScriptCollector::new(pattern)),
                    }),
                    reducer: Box::new(CustomScriptReducer {}),
                },
            ),
        ]
        .iter()
        .cloned()
        .collect()
    })
}

pub struct Stat {
    pub measurement_kind: MeasurementKind,
    pub blob_measurer: Box<dyn BlobMeasurer<F> + Send + Sync>,
    pub reducer: Box<dyn Reducer<F> + Send + Sync>,
}
impl Stat {
    pub fn from_config(config: &StatConfig) {
        let stat_name = config.name;
        let Ok(stat_kind) = name.parse() else {
            panic!(
                "{stat_name} is not a valid stat name. Known stat names are: '{}'",
                BuiltinStat::VARIANTS.join(", ")
            );
        };
        match stat_kind {
            BuiltinStat::Tokei => {
                let blob_measurer = Box::new(FileContentsMeasurer {
                    callback: Box::new(TokeiCollector::new()),
                });
                Stat {
                    measurement_kind: MeasurementKind::FileContents,
                    blob_measurer,
                    reducer,
                }
            }
            BuiltinStat::Grep => {
                let blob_measurer = Box::new(FileContentsMeasurer {
                    callback: Box::new(RipgrepCollector::new(pattern)),
                });
                Stat {
                    measurement_kind: MeasurementKind::FileContents,
                    blob_measurer,
                    reducer,
                }
            }
            BuiltinStat::FileCount => {
                let blob_measurer = Box::new(FilePathMeasurer {
                    callback: Box::new(PathBlobCollector::new(pattern)),
                });
                Stat {
                    measurement_kind: MeasurementKind::FilePath,
                    blob_measurer,
                }
            }
            BuiltinStat::Custom => {
                let blob_measurer = Box::new(CustomScriptMeasurer {
                    callback: Box::new(CustomScriptCollector::new(pattern)),
                });
                let reducer = Box::new(CustomScriptReducer {});
                Stat {
                    measurement_kind: MeasurementKind::CustomScript,
                    blob_measurer,
                    reducer,
                }
            }
        }
    }
}
