use criterion::{black_box, criterion_group, criterion_main, Criterion};
use unleash_edge::{
    tokens::simplify,
    types::{EdgeToken, TokenType},
};

fn test_token(env: Option<&str>, projects: Vec<&str>) -> EdgeToken {
    EdgeToken {
        secret: "the-secret".into(),
        token_type: Some(TokenType::Client),
        environment: env.map(|env| env.into()),
        projects: projects.into_iter().map(|p| p.into()).collect(),
        expires_at: None,
        seen_at: None,
        alias: None,
    }
}

fn bench_simplify(c: &mut Criterion) {
    let tokens = vec![
        test_token(None, vec!["p1", "p2"]),
        test_token(None, vec!["p1"]),
        test_token(None, vec!["*"]),
        test_token(None, vec!["p3"]),
        test_token(Some("env"), vec!["p1", "p2"]),
        test_token(Some("env"), vec!["p1"]),
        test_token(Some("env"), vec!["p2"]),
        test_token(Some("env"), vec!["p2", "p3"]),
    ];

    c.bench_function("simplify_bench", |b| {
        b.iter(|| simplify(black_box(&tokens)))
    });
}

criterion_group!(benches, bench_simplify);
criterion_main!(benches);
