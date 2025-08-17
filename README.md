# Postgres With QUIC

An attempt to use postgres protocol over QUIC(UDP) instead of TCP

pgcat has been modified to support QUIC: https://github.com/ash-hashtag/pgcat

default pooler establishes new TCP connection per client and reuses them
a single pooler over QUIC uses same existing connection with new stream per client and reuses them


## But Why?

Postgres(protocol) is synchronous, that is you can only run one query at a time in a connection, thats why pools and poolers are used to get around

A pool basically makes multiple synchronous connections and allows queries to run in parallel on different connections

For example, if an edge server has to process 100 requests at a time, and would require 100 database queries, it would have to establish 100 connections.
It doesn't scale, you stop at a max_pool_size, and wait until a connection in pool is done with its query, hence increasing response times over time with more and more queries.

Therefore using a single connection with concurrent queries is obviously a better choice, so why not http2 like TCP connection with multiplexing, and allow concurrent streams, Well not a bad idea, no real reason, I just wanted to have a look at QUIC, maybe I can see QUIC/UDP saves bandwidth and gives better latency, blah blah blah. But actually the default performance is actually worse than TCP, until we are hitting the limits of TCP.

But anyway, the advantage is at using one single connection with multiple streams and we have achieved it

using TCP with 1000 connections hits us with "too many open files", well thats one disadvantage of using it, which you can get around

But... using one connection with 1000 streams has an advantage of not even thinking about the limit

So with the default behaviour you can never get 1000+ concurrent queries (on regular clients), assuming the database can handle them

for example, a roundtrip from a client to pooler might be around 100ms, but from pooler to database would be around 1ms

So the concurrency/parallelism would drop even lower even with using a pool, a client in the pool can't be freed for the next request until 100ms

And it becomes very inefficient to have 1000 active but just waiting TCP connections to same server

So using QUIC would allow us to even open up millions (not necessary) of streams split over multiple connections, and it is the pooler on that side's problem how to deal with that large number of streams, which probably can be further optimized, as right now the updated pgcat, just takes QUIC streams and passes onto the existing stream handler (tcp) 


As from the collected metrics, for less number of connections we can have more concurrent queries with lower latency using QUIC compared to the default TCP. The pool method can be further optimized


Here is a comparison using 500 TCP connections vs 50 QUIC connections with 40 streams with the settings from .env.quic .env.tcp files in the repo

![Quic-Tcp-Comparision](/metrics/tcp-quic-comparison-metrics.webp)

Overall CPU usage is higher with QUIC than TCP, that is expected, as we are processing more queries than default anyway
But the latency and memory usage shows the obvious benifit

Or better yet? update the postgres protocol (make a different version) to have concurrent queries, then we don't have to find work arounds like these just to be able to handle multiple queries at once

But In my opinion, It was much simpler than I expected, QUIC can be just drop in placed, to have Clients and Poolers to support it for better overall performance.

This is not about TCP vs QUIC, I've just used the terms to best fit the context.
