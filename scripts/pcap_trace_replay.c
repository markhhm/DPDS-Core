/* SPDX-License-Identifier: Apache-2.0 */
/* Optional deterministic PMD adapter. Payload arrival clocks are independent of pcap timestamps. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_errno.h>
#include "libdpds_core.h"

static FILE *records;
static size_t emitted;
static uint64_t decode(const unsigned char *p) {
    uint64_t value = 0;
    for (unsigned i = 0; i < 8; ++i) value |= (uint64_t)p[i] << (8*i);
    return value;
}
static void drain(struct Engine *engine, uint64_t now) {
    struct HngPacket event;
    while (hng_engine_pop(engine, 0, now, now, &event)) {
        struct rte_mbuf *packet = event.packet_ptr;
        uint64_t id = decode(rte_pktmbuf_mtod(packet, const unsigned char *) + 18);
        fprintf(records, "%s{\"id\":%"PRIu64",\"ingress_seq\":%"PRIu64
            ",\"service_ns\":%"PRIu64",\"deadline_ns\":%"PRIu64
            ",\"release_ns\":%"PRIu64",\"clamp_extra_ns\":%"PRIu64",\"late\":%s}",
            emitted ? "," : "", id, event.ingress_seq, event.service_ns,
            event.deadline_ns, event.release_ns, event.clamp_extra_ns, event.late ? "true" : "false");
        assert(rte_eth_tx_burst(0, 0, &packet, 1) == 1);
        emitted++;
    }
}
int main(int argc, char **argv) {
    assert(getenv("PCAP_CONFIG") && getenv("PCAP_COUNT") && getenv("PCAP_RECORDS"));
    size_t count = strtoull(getenv("PCAP_COUNT"), NULL, 10);
    assert(count > 0 && count <= 1000000);
    struct Engine *engine = hng_engine_load(getenv("PCAP_CONFIG"));
    assert(engine && hng_engine_line_count(engine) == 1);
    assert(rte_eal_init(argc, argv) >= 0);
    assert(rte_eth_dev_count_avail() == 1);
    struct rte_mempool *pool = rte_pktmbuf_pool_create("replay", count+1024, 0, 0,
                                                     RTE_MBUF_DEFAULT_BUF_SIZE, SOCKET_ID_ANY);
    if (!pool) {
        fprintf(stderr, "mempool allocation for %zu packets failed: %s\n", count, rte_strerror(rte_errno));
        return 1;
    }
    struct rte_eth_conf conf = {0};
    assert(rte_eth_dev_configure(0, 1, 1, &conf) == 0);
    assert(rte_eth_rx_queue_setup(0, 0, 128, SOCKET_ID_ANY, NULL, pool) == 0);
    assert(rte_eth_tx_queue_setup(0, 0, 128, SOCKET_ID_ANY, NULL) == 0);
    assert(rte_eth_dev_start(0) == 0);
    records = fopen(getenv("PCAP_RECORDS"), "wx"); assert(records);
    fputc('[', records);
    size_t received = 0;
    uint64_t previous = 0;
    while (received < count) {
        struct rte_mbuf *batch[32];
        unsigned n = rte_eth_rx_burst(0, 0, batch, 32);
        assert(n > 0 && received+n <= count);
        for (unsigned i = 0; i < n; ++i) {
            struct rte_mbuf *m = batch[i];
            assert(m->pkt_len == 76 && rte_pktmbuf_is_contiguous(m));
            const unsigned char *data = rte_pktmbuf_mtod(m, const unsigned char *);
            uint64_t id = decode(data+18), arrival = decode(data+26);
            assert(id == received+1 && (received == 0 || arrival > previous));
            assert(hng_engine_offer(engine, 0, m, m->pkt_len, arrival, received) == Accepted);
            drain(engine, arrival);
            previous = arrival;
            received++;
        }
    }
    struct rte_mbuf *extra;
    assert(rte_eth_rx_burst(0, 0, &extra, 1) == 0);
    hng_engine_end_input(engine);
    drain(engine, UINT64_MAX);
    assert(emitted == count);
    assert(hng_engine_drain_advance(engine, UINT64_MAX));
    assert(!hng_engine_has_backlog(engine));
    assert(hng_engine_destroy(engine));
    fputs("]\n", records); assert(fclose(records) == 0);
    assert(rte_eth_dev_stop(0) == 0);
    assert(rte_eth_dev_close(0) == 0);
    rte_mempool_free(pool);
    assert(rte_eal_cleanup() == 0);
    return 0;
}
