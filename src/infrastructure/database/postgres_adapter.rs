use sqlx::postgres::PgPool;

pub struct PostgresRepository {
    pub(crate) pool: PgPool,
}

impl PostgresRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
