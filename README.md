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

This is not a comparison between TCP or UDP or QUIC, QUIC is an application level protocol over UDP
Similar results can be acheived using a protocol with multiplexing streams over TCP similar to http2, I just chose to use QUIC

This is only discussing the bottleneck for using synchronous postgres protocol per connection and spending time for just "waiting" instead of processing more queries in parallel

We are not modifying the postgres protocol anyway, it is still synchronous, we are just making more "virtual" connections over the same connection, increasing our throughput

using the default TCP with 1000 connections hits us with "too many open files", well thats one disadvantage of using it, which you can get around

But... using one connection with 1000 streams has an advantage of not even thinking about the limit

So with the default behaviour you can never get 1000+ concurrent queries (on regular clients), assuming the database can handle them

for example, a roundtrip from a client to pooler might be around 100ms, but from pooler to database would be around 1ms

So the concurrency/parallelism would drop even lower even with using a pool, a client in the pool can't be freed for the next request until 100ms

And it becomes very inefficient to have 1000 active but just waiting TCP connections to the same server

So using QUIC would allow us to even open up millions (not necessary) of streams split over multiple connections, and it is the pooler on that side's problem how to deal with that large number of streams, which probably can be further optimized, as right now the updated pgcat, just takes QUIC streams and passes onto the existing stream handler (tcp) 


As from the collected metrics, for far less number of connections we can have more concurrent queries with lower latency using QUIC compared to the default TCP. The pool method can be further optimized


Here is a comparison using 500 TCP connections vs 50 QUIC connections with 40 streams with the settings from .env.quic .env.tcp files in the repo

![Quic-Tcp-Comparision](/metrics/tcp-quic-comparison-metrics.webp)

Its not even a fair comparison

Overall CPU usage is higher with QUIC than TCP, that is expected, as we are processing more queries than default anyway
But the latency and memory usage shows the obvious benefit

But In my opinion, It was much simpler than I expected to use QUIC alongside, QUIC can be just drop in placed, to have Clients and Poolers support it for better overall performance.

Even the poolers would not need to worry about "too many files", but the memory and cpu usage I'd expect to be pretty similar tho, QUIC isn't anymore efficient

