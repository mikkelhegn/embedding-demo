use anyhow::{Context, Result};
use log::{info, trace, LevelFilter::Info, error};
use serde::{Deserialize, Serialize};
use serde_json::*;
use spin_sdk::{
    http::{Params, Request, Response},
    http_component, http_router,
    llm::{generate_embeddings, EmbeddingModel::AllMiniLmL6V2, EmbeddingsResult},
    sqlite::{self, Connection, ValueResult},
};

#[http_component]
fn handle_request(req: Request) -> Result<Response> {
    env_logger::builder().filter_level(Info).init();

    info!(
        "Received {} request at {}",
        req.method().to_string(),
        req.uri().to_string()
    );

    let router = http_router! {
        GET "/embeddings" => get_paragraphs,
        POST "/embeddings" => create_paragraphs_records,
        DELETE "/embeddings/:reference" => delete_paragraph_record,
        _ "/*" => |_req, _params| {
            Ok(http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .body(None)
                .unwrap())
        }
    };

    router.handle(req)
}

fn get_paragraphs(req: Request, _params: Params) -> Result<Response> {
    match req.uri().query() {
        Some(query) => {
            let query: Query = serde_qs::from_str(query)?;
            let result_set = get_similar_paragraphs(&query.sentence)?;

            Ok(http::Response::builder()
                .status(http::StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Some(serde_json::to_vec(&result_set)?.into()))?)
        }
        None => {
            let query = "SELECT * FROM paragraphs";
            let conn = Connection::open_default()?;
            let all_paragraphs: Vec<Paragraph> = match conn
                .execute(query, &[])?
                .rows()
                .map(|row| -> anyhow::Result<Paragraph> { row.try_into() })
                .collect::<anyhow::Result<Vec<Paragraph>>>() {
                    Ok(p) => {
                        trace!("All paragraphs: {:?}", p);
                        p
                    }
                    Err(err) => {
                        error!("Error getting paragraphs from db: {:?}", err);
                        return Ok(http::Response::builder()
                            .status(http::StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Some("Failed to get records".into()))?)
                    }
                };

            Ok(http::Response::builder()
                .status(http::StatusCode::OK)
                .body(Some(serde_json::to_string(&all_paragraphs)?.into()))?)
        }
    }
}

fn create_paragraphs_records(req: Request, _params: Params) -> Result<Response> {
    let paragraphs: Vec<Paragraph> = match serde_json::from_slice(
            req.body()
                .as_deref()
                .map(|b| -> &[u8] { b })
                .unwrap_or_default()) {
        Ok(vec) => vec,
        Err(err) => {
            error!("Failed to serialize paragraphs");
            return Err(err.into())
        },
    };

    let text: Vec<&str> = paragraphs.iter().map(|e| e.text.as_str()).collect();
    let embedding_result: EmbeddingsResult = match generate_embeddings(AllMiniLmL6V2, &text) {
        Ok(er) => {
            trace!("Generated embeddings: {:?}", er);
            er
        },
        Err(err) => { 
            error!("Failed to generate embeddings when calling Spin llm: {:?}", err);
            return Err(err.into());
        }
    };

    match store_paragraph_records(paragraphs, embedding_result) {
        Ok(num_rec) => {
            info!("Generated {:?} embeddings", num_rec);
            Ok(http::Response::builder()
            .status(http::StatusCode::CREATED)
            .body(Some(format!("Stored {:?} records", num_rec).into()))?)
        }
        Err(err) => {
            error!("Failed to store records: {:?}", err);
            Ok(http::Response::builder()
            .status(http::StatusCode::INTERNAL_SERVER_ERROR)
            .body(Some("Failed to store records".into()))?)
        }
    }
}

fn store_paragraph_records(paragraphs: Vec<Paragraph>, embedding_result: EmbeddingsResult) -> Result<usize> {
    let conn = Connection::open_default()?;

    for (e, res) in paragraphs.iter().zip(embedding_result.embeddings) {
        let vec = json!(res.clone());
        let blob = serde_json::to_vec(&vec)?;

        let query_params = [
            sqlite::ValueParam::Text(e.reference.as_str()),
            sqlite::ValueParam::Text(e.text.as_str()),
            sqlite::ValueParam::Blob(blob.as_slice()),
        ];

        let _ = conn.execute(
            "INSERT INTO embeddings ('reference', 'text', 'embedding') VALUES (?, ?, ?);",
            &query_params,
        );
    }

    Ok(paragraphs.len())
}

fn delete_paragraph_record(_req: Request, params: Params) -> Result<Response> {
    let status = match params.get("reference") {
        Some(reference) => {
            let query_params = [sqlite::ValueParam::Text(reference)];
            let conn = Connection::open_default()?;
            let _ = conn.execute("DELETE FROM paragraphs WHERE reference = (?)", &query_params);
            info!("Deleted one record");
            http::StatusCode::OK
        }
        None => http::StatusCode::NOT_FOUND,
    };

    Ok(http::Response::builder().status(status).body(None)?)
}

fn get_similar_paragraphs(sentence: &str) -> Result<SimilarityResultSet> {
    let paragraphs = get_compare_set()?;

    let embedded_sentence: Vec<f32> = match generate_embeddings(AllMiniLmL6V2, &[sentence]) {
        Ok(er) => {
            trace!("Generated embeddings: {:?}", er);
            er.embeddings.get(0).unwrap().to_vec()
        },
        Err(err) => { 
            error!("Failed to generate embeddings when calling Spin llm: {:?}", err);
            return Err(err.into())
        }
    };

    let mut results: Vec<SimilarityResult> = paragraphs.into_iter().map(|p|  SimilarityResult {
                similarity: cosine_similarity(p.embedding.as_ref(), embedded_sentence.as_ref()),
                paragraph: Paragraph {
                    reference: p.reference,
                    text: p.text,
                }
            })
            .collect();

    results.sort_by(|a, b| {
        b.similarity.partial_cmp(&a.similarity).unwrap()
    });

    let similarity_results = SimilarityResultSet {
        sentence: sentence.to_string(),
        results,
    };

    Ok(similarity_results)
}

fn get_compare_set() -> Result<Vec<ParagraphRecord>> {
    let sql_query = "SELECT * FROM embeddings";
    match Connection::open_default()?
        .execute(sql_query, &[])?
        .rows()
        .map(|row| -> anyhow::Result<ParagraphRecord> { row.try_into() })
        .collect::<anyhow::Result<Vec<ParagraphRecord>>>() {
            Ok(er) => {
                Ok(er)
            },
            Err(err) => {
                error!("Failed to get paragraphs to compare with");
                Err(err)
            },
        }
}

fn cosine_similarity(vec1: &[f32], vec2: &[f32]) -> f32 {
    let dot_product = vec1
        .iter()
        .zip(vec2.iter())
        .map(|(x, y)| x * y)
        .sum::<f32>();
    let norm1 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm2 = vec2.iter().map(|y| y * y).sum::<f32>().sqrt();
    dot_product / (norm1 * norm2)
}

impl<'a> TryFrom<sqlite::Row<'a>> for ParagraphRecord {
    type Error = anyhow::Error;

    fn try_from(row: sqlite::Row<'a>) -> std::result::Result<Self, Self::Error> {
        let embedding: Vec<f32> = match row.get::<&ValueResult>("embedding").unwrap() {
            ValueResult::Blob(b) => {
                serde_json::from_value(serde_json::from_slice(b.as_slice()).unwrap_or_default())?
            }
            _ => todo!(),
        };
        let reference = row
            .get::<&str>("reference")
            .context("reference column is empty")?;
        let text = row.get::<&str>("text").context("text column is empty")?;
        Ok(Self {
            reference: reference.to_owned(),
            text: text.to_owned(),
            embedding,
        })
    }
}

impl<'a> TryFrom<sqlite::Row<'a>> for Paragraph {
    type Error = anyhow::Error;

    fn try_from(row: sqlite::Row<'a>) -> std::result::Result<Self, Self::Error> {
        let reference = row
            .get::<&str>("reference")
            .context("reference column is empty")?;
        let text = row.get::<&str>("text").context("text column is empty")?;
        Ok(Self {
            reference: reference.to_owned(),
            text: text.to_owned(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Paragraph {
    reference: String,
    text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ParagraphRecord {
    embedding: Vec<f32>,
    reference: String,
    text: String,
}

#[derive(Serialize)]
struct SimilarityResultSet {
    sentence: String,
    results: Vec<SimilarityResult>,
}

#[derive(Serialize)]
struct SimilarityResult {
    paragraph: Paragraph,
    similarity: f32,
}

#[derive(Deserialize)]
struct Query {
    sentence: String,
}
