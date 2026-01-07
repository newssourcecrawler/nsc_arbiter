#pragma once
#include <stdint.h>
#include <stddef.h>

// nsc_arbiter_ffi ABI version.
// Bumped when any exported function signature or struct layout changes.
#define NSC_ARBITER_FFI_VERSION 1

#ifdef __cplusplus
extern "C" {
#endif

typedef struct NscArbiterSupervisor NscArbiterSupervisor;

typedef struct { const uint8_t* ptr; size_t len; } NscStr; // UTF-8 bytes view (ptr may be NULL)

typedef struct { NscStr key; float val; } NscScalarKV;

typedef struct {
  NscStr intent_id;
  NscStr source_id;
  NscStr origin;
  NscStr text; // optional: ptr may be NULL
  size_t scalars_len;
  const NscScalarKV* scalars_ptr;
  uint32_t rule_hits;
} NscEvent;

typedef enum {
  NSC_ESC_NONE = 0,
  NSC_ESC_CRITIQUE_PASS = 1,
  NSC_ESC_SECOND_LLM = 2
} NscEscalation;

typedef struct {
  NscStr intent_id;
  NscEscalation escalation;
  float avg_entropy;
  float cosine_sim;
  float gate_shift;
  uint32_t rule_hits;
  uint8_t ff_rep_3p;
  uint8_t ff_stall;
  uint8_t ff_ai_tell;
} NscAction;

typedef struct {
  NscAction* actions_ptr;
  size_t actions_len;
  uint8_t* strings_ptr;
  size_t strings_len;
} NscActionArray;

typedef struct { uint8_t* ptr; size_t len; } NscBytes;

typedef struct {
  float tau_e;
  float tau_s;
  uint32_t tau_rep;
  uint32_t tau_stall;
  float tau_gate;
  uint8_t hyst_disable;
  int32_t forced_rule_hits; // -1 means None
} NscCfg;

// Returns the ABI version implemented by the linked library.
uint32_t nsc_arbiter_ffi_version(void);

// Returns a default configuration matching Rust `ArbiterCfg::default()`.
NscCfg nsc_arbiter_cfg_default(void);

NscArbiterSupervisor* nsc_arbiter_supervisor_new(size_t shards, NscCfg cfg);
void nsc_arbiter_supervisor_free(NscArbiterSupervisor* h);

NscActionArray nsc_arbiter_ingest(NscArbiterSupervisor* h, const NscEvent* events_ptr, size_t events_len);
void nsc_arbiter_actions_free(NscActionArray arr);

// Snapshot bytes are a versioned binary format (magic+version prefix).
// Use nsc_arbiter_restore() to restore into a supervisor.
NscBytes nsc_arbiter_snapshot(NscArbiterSupervisor* h);
void nsc_arbiter_bytes_free(NscBytes b);

// Restore a snapshot returned by nsc_arbiter_snapshot().
// merge=0: clear then load; merge!=0: overlay into existing state.
// Returns 0 on success, negative error codes on failure.
int32_t nsc_arbiter_restore(NscArbiterSupervisor* h, const uint8_t* bytes, size_t len, uint8_t merge);

#ifdef __cplusplus
} // extern "C"
#endif