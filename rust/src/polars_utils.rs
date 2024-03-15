
use polars::{
    datatypes::AnyValue,
    error::PolarsError,
    frame::{
        row::{Row},
        DataFrame,
    },
    prelude::Schema,
};

/// Convert a list of rows and schemas to a DataFrame
/// If a row has a different list of columns than the others,
/// that column gets added to all the rows.
/// This should be in polars but isn't.
pub fn rows_to_df<'a>(
    schemas: impl Iterator<Item = Schema>,
    rows: impl Iterator<Item = &'a Row<'a>>,
) -> Result<DataFrame, PolarsError> {
    let all_schemas = schemas.collect::<Vec<_>>();
    let all_schemas_identical = all_schemas.iter().all(|schema| *schema == all_schemas[0]);
    if all_schemas_identical {
        return DataFrame::from_rows_iter_and_schema(rows, &all_schemas[0]);
    }
    // There's an alternative here with infer_schema, but it's ugly
    let mut full_schema = Schema::new();
    for schema in all_schemas.iter() {
        full_schema.merge(schema.clone());
    }
    // alternative looks like this:
    // let full_schema = infer_schema(
    //     all_schemas.iter().map(|schema| {
    //         schema
    //             .iter_fields()
    //             .map(|field| (field.name().to_string(), field.dtype))
    //             .collect::<Vec<_>>()
    //     }),
    //     all_schemas.len(),
    // );
    let expanded_rows = all_schemas
        .iter()
        .zip(rows)
        .map(|(schema, row)| {
            let vals = full_schema
                .iter()
                .map(|field| match schema.get_full(field.0) {
                    Some((col_idx_in_this_row, _, _)) => row
                        .0
                        .get(col_idx_in_this_row)
                        .cloned()
                        .unwrap_or_else(AnyValue::default),
                    None => AnyValue::default(),
                })
                .collect::<Vec<_>>();
            Row::new(vals)
        })
        .collect::<Vec<_>>();

    DataFrame::from_rows_and_schema(&expanded_rows, &full_schema)
}
