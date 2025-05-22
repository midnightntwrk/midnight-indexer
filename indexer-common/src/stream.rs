use futures::{Stream, StreamExt, stream};
use std::iter;

/// Flattens a stream of results of chunks of items into a stream of results of items.
pub fn flatten_chunks<T, E>(
    chunks: impl Stream<Item = Result<Vec<T>, E>>,
) -> impl Stream<Item = Result<T, E>> {
    chunks.flat_map(|chunk: Result<Vec<_>, E>| match chunk {
        Ok(chunk) => stream::iter(chunk.into_iter().map(Ok)).left_stream(),
        Err(error) => stream::iter(iter::once(Err(error))).right_stream(),
    })
}

#[cfg(test)]
mod tests {
    use crate::stream::flatten_chunks;
    use assert_matches::assert_matches;
    use futures::{TryStreamExt, stream};
    use std::convert::Infallible;

    #[tokio::test]
    async fn test_flatten_chunks() {
        let chunks = stream::iter(vec![Ok::<_, Infallible>(vec![1, 2, 3]), Ok(vec![4, 5, 6])]);
        let flattened = flatten_chunks(chunks).try_collect::<Vec<_>>().await;
        assert_matches!(flattened, Ok(x) if x == vec![1, 2, 3, 4, 5, 6]);

        let chunks = stream::iter(vec![Ok::<_, &'static str>(vec![1, 2, 3]), Err("error")]);
        let mut flattened = flatten_chunks(chunks);
        assert_eq!(flattened.try_next().await, Ok(Some(1)));
        assert_eq!(flattened.try_next().await, Ok(Some(2)));
        assert_eq!(flattened.try_next().await, Ok(Some(3)));
        assert_eq!(flattened.try_next().await, Err("error"));
    }
}
