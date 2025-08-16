# Postgres With QUIC

An attempt to use postgres protocol over QUIC(UDP) instead of TCP

pgcat has been modified to support QUIC: https://github.com/ash-hashtag/pgcat

default pooler establishes new TCP connection per client and reuses them
a single pooler over QUIC uses same existing connection with new stream per client and reuses them


## But Why?

Postgres(protocol) is synchronous, that is you can only run one query at a time, thats why pools and poolers are used

A pool basically makes multiple synchronous connections and allows queries to run in parallel on different connections, therefore it is just workaround

For example, if an edge server has to process 100 requests at a time, and would require 100 database queries, it would have to establish 100 connections.
It doesn't scale, you stop at a max_pool_size, and wait until an object in pool is done with its query, hence increasing response times over time with more and more queries.


Therefore using a single connection is obviously a better choice, so why not http2 like TCP connection, and allow concurrent streams, Well not a bad idea, no real reason, I just wanted to have a look at QUIC, maybe I can see QUIC/UDP saves bandwidth and gives better latency, blah blah blah. But actually the default performance is actually worse than TCP.

But anyway, the advantage is at using one single connection with multiple streams and we have achieved it

From the metrics
using TCP with 1000 connections hits with "too many open files", well thats one disadvantage of using it, which you can get around

But... using one connection with 1000 streams has an advantage of not even thinking about the limit


So with the default behaviour you can never get 1000+ concurrent queries (on regular clients), the database might throttle anyway, but all of this is under the assumption

The database queries are far far faster than connection/response time overhead

for example, a roundtrip from a client to pooler might be around 100ms, but from pooler to database would be around 1ms

So the concurrency/parallelism would drop even lower even with using a pool, a client in the pool can't be freed for the next request until 100ms

And it is unrealistic/inefficient to even have 1000 active TCP connections to same server

So using QUIC would allow us to even open up millions of streams split over multiple connections, and it is the poolers problem how to deal with them on its end, which probably can be further optimized, as right now the updated pgcat, just takes QUIC streams and passes onto the existing tcp stream handlers, hence each new stream, does have to go through the same "connection phase" and "isolation" and gets its own data, instead we can make some of the data shared, since we know, all the streams are coming from one connection

Or better yet? update the postgres protocol (make a different version) to have concurrent queries, then we don't have to jump around all of these just to be able to handle multiple queries at once

As from the collected metrics, for less number of connections we can have more concurrent queries with lower latency using QUIC compared to the default TCP. The pool method can be further optimized, as right now it just opens a new connection when previous one fills its stream quota but never reuses the connection, after its streams close
