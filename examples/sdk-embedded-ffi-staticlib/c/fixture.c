/* Independent E5 C11 consumer; actual Rust no_std Agent through FFI.
 * No Rust Agent/private types imported. This process intentionally uses the
 * C runtime for its fixture/test harness. It is NOT an E4 libc-free executable.
 */
#include "aw_embedded_ffi.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stddef.h>

static int checks=0;
#define CHECK(test) do { \
    if (!(test)) { fprintf(stderr,"E5 C FAILED line %d: %s\n",__LINE__,#test); exit(3); } \
    checks++; \
} while (0)

enum {
    CASE_OK=0, CASE_INTENT_DENIED=1, CASE_RECEIPT_FAIL=2,
    CASE_REVOKE=3, CASE_MODEL_JSON_FAIL=4, CASE_MODEL_TOO_LONG=5,
    CASE_MODEL_CALLBACK_FAIL=6, CASE_TOOL_CALLBACK_FAIL=7,
    CASE_PURE_TEXT=8, CASE_CLOCK_FAIL=9, CASE_ENTROPY_FAIL=10,
    CASE_ASSISTANT_JOURNAL_FAIL=11
};
typedef struct {
    AwE5Engine *engine;
    uint64_t ticks;
    unsigned char journal[64];
    size_t journal_count;
    int grant_active;
    int mode;
    int model_calls;
    int tool_calls;
    int authorize_calls;
    int validate_calls;
    int callback_reentry_checks;
} Context;

static void json_input(const uint8_t *input,size_t len,char *out,size_t cap) {
    CHECK(input!=NULL && len>0 && len<cap);
    memcpy(out,input,len);
    out[len]='\0';
}
static uint32_t reply(const char *json,uint8_t *out,size_t cap,size_t *written) {
    CHECK(json!=NULL && written!=NULL && out!=NULL);
    const size_t len=strlen(json);
    CHECK(len<=cap);
    memcpy(out,json,len);
    *written=len;
    return AW_E5_PORT_OK;
}
static uint32_t native_clock(void *user,uint64_t *ticks) {
    Context *ctx=(Context*)user;
    CHECK(ticks!=NULL);
    if (ctx->mode==CASE_CLOCK_FAIL)return AW_E5_PORT_UNAVAILABLE;
    *ticks=++ctx->ticks;
    return AW_E5_PORT_OK;
}
static uint32_t native_entropy(void *user,uint8_t *out,size_t len) {
    Context *ctx=(Context*)user;
    CHECK(out!=NULL && len==8);
    if (ctx->mode==CASE_ENTROPY_FAIL)return AW_E5_PORT_UNAVAILABLE;
    for(size_t i=0;i<len;i++)out[i]=(uint8_t)(i+1);
    return AW_E5_PORT_OK;
}
/* Append returns OK only after the independent host records the event.
 * Simulates a persisted+flushed journal for this C process test. */
static uint32_t native_journal(
    void *user,uint32_t kind,const uint8_t *input,size_t len
) {
    Context *ctx=(Context*)user;
    char text[256*1024];
    json_input(input,len,text,sizeof(text));
    CHECK(strstr(text,"\"turn_id\":1")!=NULL);
    CHECK(ctx->journal_count<sizeof(ctx->journal));
    if (kind==AW_E5_JOURNAL_TOOL_INTENT && ctx->mode==CASE_INTENT_DENIED) {
        return AW_E5_PORT_FAILED;
    }
    if (kind==AW_E5_JOURNAL_TOOL_RECEIPT && ctx->mode==CASE_RECEIPT_FAIL) {
        return AW_E5_PORT_FAILED;
    }
    if (kind==AW_E5_JOURNAL_ASSISTANT &&
        ctx->mode==CASE_ASSISTANT_JOURNAL_FAIL) {
        return AW_E5_PORT_FAILED;
    }
    ctx->journal[ctx->journal_count++]=(unsigned char)kind;
    if (kind==AW_E5_JOURNAL_TOOL_INTENT && ctx->mode==CASE_REVOKE) {
        ctx->grant_active=0;
    }
    return AW_E5_PORT_OK;
}
static uint32_t native_model(
    void *user,const uint8_t *input,size_t len,
    uint8_t *out,size_t cap,size_t *written
) {
    Context *ctx=(Context*)user;
    char text[2*1024*1024];
    json_input(input,len,text,sizeof(text));
    CHECK(strstr(text,"\"actor\":\"user\"")!=NULL);
    CHECK(strstr(text,"\"account\":\"acct\"")!=NULL);
    CHECK(strstr(text,"\"model\":\"fixture\"")!=NULL);
    /* Callback reentry must be refused before the Rust mutable alias; the
     * native C caller keeps a valid opaque pointer for the whole lifetime. */
    AwE5StateV1 state={0};
    state.struct_size=sizeof(state);
    state.abi_version=AW_E5_ABI_VERSION;
    CHECK(aw_e5_state(ctx->engine,&state)==AW_E5_BUSY);
    AwE5Engine *before=ctx->engine;
    CHECK(aw_e5_destroy(&ctx->engine,1)==AW_E5_BUSY);
    CHECK(ctx->engine==before);
    ctx->callback_reentry_checks+=2;
    ctx->model_calls++;
    if (ctx->mode==CASE_MODEL_CALLBACK_FAIL)return AW_E5_PORT_FAILED;
    if (ctx->mode==CASE_MODEL_JSON_FAIL)return reply("{",out,cap,written);
    if (ctx->mode==CASE_MODEL_TOO_LONG) {
        *written=cap+1;
        return AW_E5_PORT_OK; /* deliberately invalid host callback contract */
    }
    if (ctx->mode==CASE_PURE_TEXT || ctx->mode==CASE_ASSISTANT_JOURNAL_FAIL) {
        return reply("{\"text\":\"pure completion\",\"finish\":\"final\",\"calls\":[]}",
                     out,cap,written);
    }
    if (strstr(text,"\"role\":\"tool\"")!=NULL) {
        CHECK(strstr(text,"\"content\":\"ok:ffi\"")!=NULL);
        return reply("{\"text\":\"turn finished\",\"finish\":\"final\",\"calls\":[]}",
                     out,cap,written);
    }
    return reply(
        "{\"text\":\"\",\"finish\":\"tools\",\"calls\":["
        "{\"id\":\"call_one\",\"name\":\"device_echo\","
        "\"arguments\":{\"payload\":\"device\"}}]}",
        out,cap,written);
}
static uint32_t native_authorize(void *user,const uint8_t *input,size_t len) {
    Context *ctx=(Context*)user;
    char json[2048];
    json_input(input,len,json,sizeof(json));
    ctx->authorize_calls++;
    if (!ctx->grant_active)return AW_E5_PORT_DENIED;
    CHECK(strstr(json,"\"name\":\"device_echo\"")!=NULL);
    CHECK(strstr(json,"\"actor\":\"user\"")!=NULL);
    CHECK(strstr(json,"\"site\":\"rom:local\"")!=NULL);
    CHECK(strstr(json,"\"source_id\":\"device-mcp\"")!=NULL);
    CHECK(strstr(json,"\"source_epoch\":42")!=NULL);
    CHECK(strstr(json,"\"effect\":\"unknown\"")!=NULL);
    CHECK(strstr(json,"\"source_kind\":3")!=NULL);
    return AW_E5_PORT_OK;
}
static uint32_t native_validate(void *user,const uint8_t *input,size_t len) {
    Context *ctx=(Context*)user;
    char json[8192];
    json_input(input,len,json,sizeof(json));
    ctx->validate_calls++;
    if (strstr(json,"\"name\":\"device_echo\"")==NULL ||
        strstr(json,"\"payload\":\"device\"")==NULL) {
        return AW_E5_PORT_DENIED;
    }
    return AW_E5_PORT_OK;
}
static uint32_t native_tool(
    void *user,const uint8_t *input,size_t len,
    uint8_t *out,size_t cap,size_t *written
) {
    Context *ctx=(Context*)user;
    char json[8192];
    json_input(input,len,json,sizeof(json));
    CHECK(strstr(json,"\"name\":\"device_echo\"")!=NULL);
    CHECK(strstr(json,"\"payload\":\"device\"")!=NULL);
    CHECK(ctx->journal_count>=1);
    CHECK(ctx->journal[ctx->journal_count-1]==AW_E5_JOURNAL_TOOL_INTENT);
    ctx->tool_calls++;
    if (ctx->mode==CASE_TOOL_CALLBACK_FAIL)return AW_E5_PORT_FAILED;
    return reply("{\"content\":\"ok:ffi\",\"is_error\":false}",
                 out,cap,written);
}
static AwE5CallbacksV1 callbacks(Context *ctx) {
    AwE5CallbacksV1 cb={0};
    cb.struct_size=sizeof(cb);cb.abi_version=AW_E5_ABI_VERSION;
    cb.context=ctx;
    cb.clock=native_clock;cb.entropy=native_entropy;
    cb.journal=native_journal;cb.model=native_model;
    cb.tool_authorize=native_authorize;
    cb.tool_validate=native_validate;cb.tool_execute=native_tool;
    return cb;
}
static AwE5Engine *create(Context *ctx,int with_tool) {
    char actor[]="user",site[]="rom:local",account[]="acct",model[]="fixture";
    char name[]="device_echo",source[]="device-mcp";
    AwE5ToolV1 tool={0};
    tool.struct_size=sizeof(tool);tool.source_kind=AW_E5_SOURCE_EXTENSION;
    tool.effect=AW_E5_EFFECT_UNKNOWN;tool.name=(const uint8_t*)name;
    tool.name_len=strlen(name);tool.source_id=(const uint8_t*)source;
    tool.source_id_len=strlen(source);tool.source_epoch=42;
    AwE5ConfigV1 config={0};
    config.struct_size=sizeof(config);config.abi_version=AW_E5_ABI_VERSION;
    config.turn_id=1;config.actor=(const uint8_t*)actor;config.actor_len=strlen(actor);
    config.site=(const uint8_t*)site;config.site_len=strlen(site);
    config.account=(const uint8_t*)account;config.account_len=strlen(account);
    config.model=(const uint8_t*)model;config.model_len=strlen(model);
    config.max_rounds=8;config.max_calls=32;config.deadline_ticks=600000;
    config.tools=with_tool?&tool:NULL;config.tools_count=with_tool?1:0;
    AwE5CallbacksV1 cb=callbacks(ctx);
    AwE5Engine *handle=(AwE5Engine*)(uintptr_t)1; /* ensure zero on failure */
    CHECK(aw_e5_create_v1(&config,&cb,&handle)==AW_E5_OK);
    CHECK(handle!=NULL && handle!=(AwE5Engine*)(uintptr_t)1);
    ctx->engine=handle;
    /* All input buffers on this stack are about to vanish. No callback was
     * permitted at construction; Rust-owned identity/tool copies must live. */
    CHECK(ctx->model_calls==0 && ctx->tool_calls==0 && ctx->journal_count==0);
    memset(actor,'X',strlen(actor));memset(name,'X',strlen(name));
    memset(source,'X',strlen(source));
    return handle;
}
static void fresh(Context *ctx,int mode) {
    memset(ctx,0,sizeof(*ctx));
    ctx->mode=mode;ctx->grant_active=1;ctx->ticks=100;
}
static AwE5StateV1 state_of(AwE5Engine *handle) {
    AwE5StateV1 result={0};
    result.struct_size=sizeof(result);result.abi_version=AW_E5_ABI_VERSION;
    CHECK(aw_e5_state(handle,&result)==AW_E5_OK);
    CHECK(result.abi_version==AW_E5_ABI_VERSION);
    return result;
}
static void destroy(Context *ctx,uint32_t discard) {
    CHECK(aw_e5_destroy(&ctx->engine,discard)==AW_E5_OK);
    CHECK(ctx->engine==NULL);
    CHECK(aw_e5_destroy(&ctx->engine,discard)==AW_E5_OK);
}
static void start(Context *ctx) {
    const char *prompt="Please echo device";
    uint32_t progress=0;
    CHECK(aw_e5_start(ctx->engine,(const uint8_t*)prompt,strlen(prompt),&progress)==AW_E5_OK);
    CHECK(progress==AW_E5_NEEDS_MODEL);
}
static void complete_success(void) {
    Context ctx;fresh(&ctx,CASE_OK);
    (void)create(&ctx,1);
    AwE5StateV1 original=state_of(ctx.engine);
    CHECK(original.status==AW_E5_CREATED && original.nonce==0 && original.trace_length==0);
    size_t needed=999;
    CHECK(aw_e5_answer_copy(ctx.engine,NULL,0,&needed)==AW_E5_OK && needed==0);
    start(&ctx);
    CHECK(state_of(ctx.engine).nonce!=0);
    uint32_t phase=0;
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(phase==AW_E5_NEEDS_TOOL);
    CHECK(aw_e5_tool_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(phase==AW_E5_NEEDS_MODEL);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(phase==AW_E5_COMPLETED);
    AwE5StateV1 final=state_of(ctx.engine);
    CHECK(final.status==AW_E5_COMPLETED && final.needs_reconciliation==0);
    CHECK(final.answer_bytes==strlen("turn finished") && final.trace_length==10);
    uint8_t out[64]={0};
    CHECK(aw_e5_answer_copy(ctx.engine,out,1,&needed)==AW_E5_BUFFER_TOO_SMALL);
    CHECK(needed==strlen("turn finished"));
    CHECK(aw_e5_answer_copy(ctx.engine,out,sizeof(out),&needed)==AW_E5_OK);
    CHECK(memcmp(out,"turn finished",needed)==0);
    AwE5EventV1 events[32]={{0}};
    CHECK(aw_e5_trace_copy(ctx.engine,events,1,&needed)==AW_E5_BUFFER_TOO_SMALL);
    CHECK(needed==10);
    CHECK(aw_e5_trace_copy(ctx.engine,events,32,&needed)==AW_E5_OK);
    CHECK(needed==10 && events[0].kind==AW_E5_EVENT_MODEL_STARTED);
    CHECK(events[1].kind==AW_E5_EVENT_BATCH_PROPOSED && events[2].kind==AW_E5_EVENT_BATCH_ADMITTED);
    CHECK(events[3].kind==AW_E5_EVENT_TOOL_INTENT && events[4].kind==AW_E5_EVENT_INTENT_RECORDED);
    CHECK(events[5].kind==AW_E5_EVENT_EFFECT_STARTED && events[6].kind==AW_E5_EVENT_EFFECT_SETTLED);
    CHECK(events[7].kind==AW_E5_EVENT_RECEIPT_RECORDED);
    CHECK(events[8].kind==AW_E5_EVENT_MODEL_STARTED && events[9].kind==AW_E5_EVENT_TEXT_COMPLETED);
    const unsigned char sequence[]={1,2,3,4,5,6,2,3,4,7};
    CHECK(ctx.journal_count==sizeof(sequence));
    CHECK(memcmp(ctx.journal,sequence,sizeof(sequence))==0);
    CHECK(ctx.model_calls==2 && ctx.tool_calls==1);
    CHECK(ctx.authorize_calls>=3 && ctx.validate_calls>=2);
    CHECK(ctx.callback_reentry_checks==4);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_WRONG_PHASE);
    CHECK(aw_e5_cancel(ctx.engine)==AW_E5_WRONG_PHASE);
    CHECK(state_of(ctx.engine).status==AW_E5_COMPLETED);
    destroy(&ctx,0);
}
static void malformed_constructor_cases(void) {
    Context ctx;fresh(&ctx,CASE_OK);
    AwE5CallbacksV1 cb=callbacks(&ctx);
    const uint8_t actor[]="user",site[]="rom:local",account[]="acct",model[]="fixture";
    const uint8_t name[]="device_echo",source[]="device-mcp";
    AwE5ToolV1 tools[2]={{0},{0}};
    for(size_t i=0;i<2;i++){
        tools[i].struct_size=sizeof(tools[i]);
        tools[i].source_kind=AW_E5_SOURCE_EXTENSION;
        tools[i].effect=AW_E5_EFFECT_UNKNOWN;
        tools[i].name=name;tools[i].name_len=sizeof(name)-1;
        tools[i].source_id=source;tools[i].source_id_len=sizeof(source)-1;
        tools[i].source_epoch=42;
    }
    AwE5ConfigV1 cfg={0};
    cfg.struct_size=sizeof(cfg);cfg.abi_version=AW_E5_ABI_VERSION;
    cfg.turn_id=1;cfg.actor=actor;cfg.actor_len=sizeof(actor)-1;
    cfg.site=site;cfg.site_len=sizeof(site)-1;
    cfg.account=account;cfg.account_len=sizeof(account)-1;
    cfg.model=model;cfg.model_len=sizeof(model)-1;
    cfg.max_rounds=8;cfg.max_calls=32;cfg.deadline_ticks=600000;
    cfg.tools=tools;cfg.tools_count=1;
    AwE5Engine *handle=(AwE5Engine*)(uintptr_t)1;
    cfg.abi_version=99;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_ABI_MISMATCH && handle==NULL);
    cfg.abi_version=AW_E5_ABI_VERSION;
    cfg.struct_size=(uint32_t)(sizeof(cfg)-1);
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_ABI_MISMATCH && handle==NULL);
    cfg.struct_size=sizeof(cfg);cb.abi_version=99;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_ABI_MISMATCH && handle==NULL);
    cb.abi_version=AW_E5_ABI_VERSION;
    cb.model=NULL;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_UNAVAILABLE && handle==NULL);
    cb=callbacks(&ctx);
    cfg.tools_count=AW_E5_MAX_TOOLS+1u;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_INVALID && handle==NULL);
    cfg.tools_count=2;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_DENIED && handle==NULL);
    cfg.tools_count=1;
    tools[0].struct_size=(uint32_t)(sizeof(tools[0])+1);
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_ABI_MISMATCH && handle==NULL);
    tools[0].struct_size=sizeof(tools[0]);
    tools[0].source_kind=99u;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_INVALID && handle==NULL);
    tools[0].source_kind=AW_E5_SOURCE_EXTENSION;tools[0].source_epoch=0;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_INVALID && handle==NULL);
    tools[0].source_epoch=42;
    const uint8_t invalid_utf8[]={0xff};
    cfg.actor=invalid_utf8;cfg.actor_len=sizeof(invalid_utf8);
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_INVALID && handle==NULL);
    cfg.actor=actor;cfg.actor_len=sizeof(actor)-1;
    cb.tool_authorize=NULL;
    CHECK(aw_e5_create_v1(&cfg,&cb,&handle)==AW_E5_UNAVAILABLE && handle==NULL);
    CHECK(ctx.journal_count==0 && ctx.model_calls==0 && ctx.tool_calls==0);
}
static void negative_cases(void) {
    malformed_constructor_cases();
    /* No host callbacks from schema/layout operations. */
    CHECK(aw_e5_abi_version()==AW_E5_ABI_VERSION);
    AwE5LayoutV1 layout={0};
    CHECK(aw_e5_layout_v1(&layout)==AW_E5_OK);
    CHECK(layout.abi_version==1 && layout.config_size==sizeof(AwE5ConfigV1));
    CHECK(layout.callbacks_size==sizeof(AwE5CallbacksV1));
    CHECK(layout.tool_size==sizeof(AwE5ToolV1) && layout.state_size==sizeof(AwE5StateV1));
    CHECK(layout.event_size==sizeof(AwE5EventV1) && layout.max_tools==AW_E5_MAX_TOOLS);
    CHECK(layout.max_callback_json==AW_E5_MAX_CALLBACK_JSON);
    CHECK(sizeof(AwE5EventV1)==8);
    CHECK(aw_e5_layout_v1(NULL)==AW_E5_INVALID);
    AwE5Engine *unused=(AwE5Engine*)(uintptr_t)1;
    CHECK(aw_e5_create_v1(NULL,NULL,&unused)==AW_E5_INVALID && unused==NULL);
    CHECK(aw_e5_create_v1(NULL,NULL,NULL)==AW_E5_INVALID);

    Context ctx;fresh(&ctx,CASE_INTENT_DENIED);
    (void)create(&ctx,1);start(&ctx);
    uint32_t phase=0;
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(aw_e5_tool_step(ctx.engine,&phase)==AW_E5_UNAVAILABLE);
    CHECK(phase==AW_E5_BLOCKED_STATUS && ctx.tool_calls==0);
    CHECK(state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_WRONG_PHASE);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    CHECK(ctx.engine!=NULL);
    destroy(&ctx,1);

    fresh(&ctx,CASE_RECEIPT_FAIL);
    (void)create(&ctx,1);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(aw_e5_tool_step(ctx.engine,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.tool_calls==1 && state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_tool_step(ctx.engine,&phase)==AW_E5_WRONG_PHASE);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    destroy(&ctx,1); /* explicit host effect-inspection decision */

    fresh(&ctx,CASE_REVOKE);
    (void)create(&ctx,1);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(aw_e5_tool_step(ctx.engine,&phase)==AW_E5_DENIED);
    CHECK(ctx.tool_calls==0);
    CHECK(state_of(ctx.engine).needs_reconciliation==1);
    destroy(&ctx,1);

    fresh(&ctx,CASE_MODEL_JSON_FAIL);
    (void)create(&ctx,1);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.tool_calls==0 && phase==AW_E5_BLOCKED_STATUS);
    CHECK(state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_WRONG_PHASE);
    destroy(&ctx,1);

    fresh(&ctx,CASE_MODEL_TOO_LONG);
    (void)create(&ctx,1);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.tool_calls==0);
    CHECK(state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    destroy(&ctx,1);

    fresh(&ctx,CASE_TOOL_CALLBACK_FAIL);
    (void)create(&ctx,1);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(aw_e5_tool_step(ctx.engine,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.tool_calls==1 && state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    destroy(&ctx,1);

    fresh(&ctx,CASE_MODEL_CALLBACK_FAIL);
    (void)create(&ctx,1);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.tool_calls==0);
    CHECK(state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    destroy(&ctx,1);

    fresh(&ctx,CASE_OK);
    (void)create(&ctx,0);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_DENIED);
    CHECK(ctx.model_calls==1 && ctx.tool_calls==0);
    CHECK(state_of(ctx.engine).status==AW_E5_BLOCKED_STATUS);
    CHECK(state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    destroy(&ctx,1);

    fresh(&ctx,CASE_PURE_TEXT);
    (void)create(&ctx,0);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_OK);
    CHECK(phase==AW_E5_COMPLETED && ctx.tool_calls==0);
    destroy(&ctx,0);

    fresh(&ctx,CASE_ASSISTANT_JOURNAL_FAIL);
    (void)create(&ctx,0);start(&ctx);
    CHECK(aw_e5_model_step(ctx.engine,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.model_calls==1 && ctx.tool_calls==0);
    CHECK(ctx.journal_count==3);
    CHECK(ctx.journal[1]==AW_E5_JOURNAL_MODEL_STARTED);
    CHECK(ctx.journal[2]==AW_E5_JOURNAL_MODEL_SETTLED);
    CHECK(state_of(ctx.engine).needs_reconciliation==1);
    CHECK(aw_e5_destroy(&ctx.engine,0)==AW_E5_RECONCILIATION_REQUIRED);
    destroy(&ctx,1);

    fresh(&ctx,CASE_CLOCK_FAIL);
    (void)create(&ctx,0);
    CHECK(aw_e5_start(ctx.engine,(const uint8_t*)"ok",2,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.model_calls==0 && ctx.tool_calls==0 && ctx.journal_count==0);
    destroy(&ctx,0);

    fresh(&ctx,CASE_ENTROPY_FAIL);
    (void)create(&ctx,0);
    CHECK(aw_e5_start(ctx.engine,(const uint8_t*)"ok",2,&phase)==AW_E5_UNAVAILABLE);
    CHECK(ctx.model_calls==0 && ctx.tool_calls==0 && ctx.journal_count==0);
    destroy(&ctx,0);

    fresh(&ctx,CASE_OK);
    (void)create(&ctx,0);
    const uint8_t bad_utf8[]={0xFFu};
    CHECK(aw_e5_start(ctx.engine,bad_utf8,sizeof(bad_utf8),&phase)==AW_E5_INVALID);
    CHECK(state_of(ctx.engine).status==AW_E5_CREATED);
    CHECK(aw_e5_start(ctx.engine,NULL,100,&phase)==AW_E5_INVALID);
    uint8_t oversized_prompt[128*1024+1];
    memset(oversized_prompt,'x',sizeof(oversized_prompt));
    CHECK(aw_e5_start(ctx.engine,oversized_prompt,sizeof(oversized_prompt),&phase)
          ==AW_E5_TOO_LARGE);
    CHECK(state_of(ctx.engine).status==AW_E5_CREATED);
    AwE5StateV1 bad_state={0};
    bad_state.struct_size=sizeof(bad_state);bad_state.abi_version=99;
    CHECK(aw_e5_state(ctx.engine,&bad_state)==AW_E5_ABI_MISMATCH);
    CHECK(aw_e5_cancel(ctx.engine)==AW_E5_OK);
    CHECK(state_of(ctx.engine).status==AW_E5_BLOCKED_STATUS);
    CHECK(aw_e5_cancel(ctx.engine)==AW_E5_WRONG_PHASE);
    destroy(&ctx,0);
}
int main(void) {
    negative_cases();
    complete_success();
    printf("E5_C_ABI_NATIVE_OK checks=%d model_calls=2 tool_calls=1 events=10\n",checks);
    return 0;
}
