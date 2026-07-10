use plotnik_tests::conformance::{default_corpus_dir, generate_corpus};

#[test]
fn committed_conformance_corpus_is_current() {
    let corpus = generate_corpus().expect("conformance corpus generates");

    corpus
        .check(&default_corpus_dir())
        .expect("committed conformance corpus is current");
}
