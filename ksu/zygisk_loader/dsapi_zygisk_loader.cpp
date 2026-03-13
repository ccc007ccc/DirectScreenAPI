#include <jni.h>
#include <sys/types.h>

#include "zygisk.hpp"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

namespace {

constexpr const char* kLogPath = "/data/adb/dsapi/log/zygisk_loader.log";
constexpr const char* kDefaultAgentService = "dsapi.zygote.injector";
constexpr const char* kAgentDescriptor = "org.directscreenapi.daemon.IZygoteAgent";
constexpr const char* kAgentFeatureDaemonBinder = "daemon_binder";
constexpr const char* kAgentFeatureScopeDecider = "scope_decider";
constexpr jint kTransGetInfo = 1;
constexpr jint kTransGetDaemonBinder = 2;
constexpr jint kTransShouldInject = 3;

struct AppMeta {
  std::string package_name;
  std::string process_name;
  jint user_id = 0;
  bool isolated = false;
  bool child_zygote = false;
  bool has_data_dir = false;
};

void append_log(const std::string& line) {
  FILE* fp = std::fopen(kLogPath, "a");
  if (fp == nullptr) {
    return;
  }
  std::fputs(line.empty() ? "-" : line.c_str(), fp);
  std::fputc('\n', fp);
  std::fclose(fp);
}

void clear_exception(JNIEnv* env, const char* stage) {
  if (env == nullptr || !env->ExceptionCheck()) {
    return;
  }
  env->ExceptionClear();
  std::string line = "zygisk_loader_warn=jni_exception stage=";
  line += (stage == nullptr || stage[0] == '\0') ? "-" : stage;
  append_log(line);
}

std::string trim_ascii(const std::string& in) {
  if (in.empty()) {
    return std::string();
  }
  size_t begin = 0;
  size_t end = in.size();
  while (begin < end) {
    char c = in[begin];
    if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
      begin += 1;
      continue;
    }
    break;
  }
  while (end > begin) {
    char c = in[end - 1];
    if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
      end -= 1;
      continue;
    }
    break;
  }
  return in.substr(begin, end - begin);
}

std::string sanitize_token(std::string raw) {
  for (size_t i = 0; i < raw.size(); ++i) {
    char& c = raw[i];
    if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
      c = '_';
    }
  }
  if (raw.empty()) {
    return "-";
  }
  return raw;
}

std::string jstring_to_utf(JNIEnv* env, jstring value) {
  if (env == nullptr || value == nullptr) {
    return std::string();
  }
  const char* chars = env->GetStringUTFChars(value, nullptr);
  if (chars == nullptr) {
    clear_exception(env, "GetStringUTFChars");
    return std::string();
  }
  std::string out(chars);
  env->ReleaseStringUTFChars(value, chars);
  return out;
}

bool is_isolated_uid(jint uid) {
  if (uid < 0) {
    return false;
  }
  int app_id = uid % 100000;
  return app_id >= 99000 && app_id <= 99999;
}

std::string infer_package_name(const std::string& data_dir, const std::string& process_name) {
  std::string dir = trim_ascii(data_dir);
  while (!dir.empty() && dir.back() == '/') {
    dir.pop_back();
  }
  if (!dir.empty()) {
    size_t pos = dir.find_last_of('/');
    std::string tail = (pos == std::string::npos) ? dir : dir.substr(pos + 1);
    tail = trim_ascii(tail);
    if (!tail.empty()) {
      return tail;
    }
  }
  std::string proc = trim_ascii(process_name);
  if (!proc.empty()) {
    size_t colon = proc.find(':');
    if (colon != std::string::npos) {
      proc = proc.substr(0, colon);
    }
    proc = trim_ascii(proc);
    if (!proc.empty()) {
      return proc;
    }
  }
  return "*";
}

AppMeta read_app_meta(JNIEnv* env, zygisk::AppSpecializeArgs* args) {
  AppMeta meta;
  if (env == nullptr || args == nullptr) {
    return meta;
  }
  meta.process_name = trim_ascii(jstring_to_utf(env, args->nice_name));
  std::string data_dir = trim_ascii(jstring_to_utf(env, args->app_data_dir));
  meta.package_name = infer_package_name(data_dir, meta.process_name);
  meta.user_id = args->uid >= 0 ? args->uid / 100000 : 0;
  meta.isolated = is_isolated_uid(args->uid);
  if (args->is_child_zygote != nullptr) {
    meta.child_zygote = (*args->is_child_zygote != 0);
  }
  meta.has_data_dir = !data_dir.empty();
  return meta;
}

std::string read_env_service_name() {
  const char* raw = std::getenv("DSAPI_ZYGOTE_AGENT_SERVICE");
  if (raw == nullptr || raw[0] == '\0') {
    return std::string(kDefaultAgentService);
  }
  std::string out = trim_ascii(raw);
  if (out.empty()) {
    return std::string(kDefaultAgentService);
  }
  for (size_t i = 0; i < out.size(); ++i) {
    char c = out[i];
    if (c == ' ' || c == '\t' || c == '\r' || c == '\n') {
      return std::string(kDefaultAgentService);
    }
  }
  return out;
}

jobject query_service_binder(JNIEnv* env, const std::string& service_name) {
  if (env == nullptr || service_name.empty()) {
    return nullptr;
  }
  jclass sm_cls = env->FindClass("android/os/ServiceManager");
  if (sm_cls == nullptr) {
    clear_exception(env, "FindClass(ServiceManager)");
    return nullptr;
  }
  jmethodID get_service = env->GetStaticMethodID(
      sm_cls,
      "getService",
      "(Ljava/lang/String;)Landroid/os/IBinder;");
  if (get_service == nullptr) {
    clear_exception(env, "GetStaticMethodID(getService)");
    env->DeleteLocalRef(sm_cls);
    return nullptr;
  }
  jstring jservice = env->NewStringUTF(service_name.c_str());
  if (jservice == nullptr) {
    clear_exception(env, "NewStringUTF(service)");
    env->DeleteLocalRef(sm_cls);
    return nullptr;
  }
  jobject binder = env->CallStaticObjectMethod(sm_cls, get_service, jservice);
  clear_exception(env, "CallStaticObjectMethod(getService)");
  env->DeleteLocalRef(jservice);
  env->DeleteLocalRef(sm_cls);
  return binder;
}

struct ParcelApi {
  jclass parcel_cls = nullptr;
  jclass ibinder_cls = nullptr;
  jmethodID parcel_obtain = nullptr;
  jmethodID parcel_recycle = nullptr;
  jmethodID write_interface_token = nullptr;
  jmethodID write_string = nullptr;
  jmethodID write_int = nullptr;
  jmethodID read_exception = nullptr;
  jmethodID read_int = nullptr;
  jmethodID read_string = nullptr;
  jmethodID create_string_array = nullptr;
  jmethodID read_strong_binder = nullptr;
  jmethodID binder_transact = nullptr;
};

bool resolve_parcel_api(JNIEnv* env, ParcelApi* api) {
  if (env == nullptr || api == nullptr) {
    return false;
  }
  *api = ParcelApi{};
  api->parcel_cls = env->FindClass("android/os/Parcel");
  if (api->parcel_cls == nullptr) {
    clear_exception(env, "FindClass(Parcel)");
    return false;
  }
  api->ibinder_cls = env->FindClass("android/os/IBinder");
  if (api->ibinder_cls == nullptr) {
    clear_exception(env, "FindClass(IBinder)");
    env->DeleteLocalRef(api->parcel_cls);
    api->parcel_cls = nullptr;
    return false;
  }

  api->parcel_obtain = env->GetStaticMethodID(api->parcel_cls, "obtain", "()Landroid/os/Parcel;");
  api->parcel_recycle = env->GetMethodID(api->parcel_cls, "recycle", "()V");
  api->write_interface_token = env->GetMethodID(api->parcel_cls, "writeInterfaceToken", "(Ljava/lang/String;)V");
  api->write_string = env->GetMethodID(api->parcel_cls, "writeString", "(Ljava/lang/String;)V");
  api->write_int = env->GetMethodID(api->parcel_cls, "writeInt", "(I)V");
  api->read_exception = env->GetMethodID(api->parcel_cls, "readException", "()V");
  api->read_int = env->GetMethodID(api->parcel_cls, "readInt", "()I");
  api->read_string = env->GetMethodID(api->parcel_cls, "readString", "()Ljava/lang/String;");
  api->create_string_array = env->GetMethodID(api->parcel_cls, "createStringArray", "()[Ljava/lang/String;");
  api->read_strong_binder = env->GetMethodID(api->parcel_cls, "readStrongBinder", "()Landroid/os/IBinder;");
  api->binder_transact = env->GetMethodID(api->ibinder_cls, "transact", "(ILandroid/os/Parcel;Landroid/os/Parcel;I)Z");

  bool ok = api->parcel_obtain != nullptr
      && api->parcel_recycle != nullptr
      && api->write_interface_token != nullptr
      && api->write_string != nullptr
      && api->write_int != nullptr
      && api->read_exception != nullptr
      && api->read_int != nullptr
      && api->read_string != nullptr
      && api->create_string_array != nullptr
      && api->read_strong_binder != nullptr
      && api->binder_transact != nullptr;
  if (!ok) {
    clear_exception(env, "GetMethodID(ParcelApi)");
    if (api->parcel_cls != nullptr) {
      env->DeleteLocalRef(api->parcel_cls);
      api->parcel_cls = nullptr;
    }
    if (api->ibinder_cls != nullptr) {
      env->DeleteLocalRef(api->ibinder_cls);
      api->ibinder_cls = nullptr;
    }
    return false;
  }
  return true;
}

void release_parcel_api(JNIEnv* env, ParcelApi* api) {
  if (env == nullptr || api == nullptr) {
    return;
  }
  if (api->parcel_cls != nullptr) {
    env->DeleteLocalRef(api->parcel_cls);
    api->parcel_cls = nullptr;
  }
  if (api->ibinder_cls != nullptr) {
    env->DeleteLocalRef(api->ibinder_cls);
    api->ibinder_cls = nullptr;
  }
}

bool transact_should_inject(JNIEnv* env,
                            jobject agent_binder,
                            const AppMeta& app,
                            std::string* reason_out) {
  if (reason_out != nullptr) {
    *reason_out = "agent_not_ready";
  }
  if (env == nullptr || agent_binder == nullptr) {
    return false;
  }

  ParcelApi api;
  if (!resolve_parcel_api(env, &api)) {
    if (reason_out != nullptr) {
      *reason_out = "parcel_api_missing";
    }
    return false;
  }

  jobject data = env->CallStaticObjectMethod(api.parcel_cls, api.parcel_obtain);
  clear_exception(env, "Parcel.obtain(data)");
  jobject reply = env->CallStaticObjectMethod(api.parcel_cls, api.parcel_obtain);
  clear_exception(env, "Parcel.obtain(reply)");
  if (data == nullptr || reply == nullptr) {
    if (reason_out != nullptr) {
      *reason_out = "parcel_alloc_failed";
    }
    if (data != nullptr) {
      env->DeleteLocalRef(data);
    }
    if (reply != nullptr) {
      env->DeleteLocalRef(reply);
    }
    release_parcel_api(env, &api);
    return false;
  }

  bool allow = false;
  std::string reason = "should_inject_transact_failed";
  jstring jdesc = nullptr;
  jstring jpkg = nullptr;
  jstring jproc = nullptr;

  jdesc = env->NewStringUTF(kAgentDescriptor);
  jpkg = env->NewStringUTF(app.package_name.c_str());
  jproc = env->NewStringUTF(app.process_name.c_str());
  if (jdesc == nullptr || jpkg == nullptr || jproc == nullptr) {
    clear_exception(env, "NewStringUTF(should_inject)");
    reason = "jni_string_alloc_failed";
    goto cleanup;
  }

  env->CallVoidMethod(data, api.write_interface_token, jdesc);
  env->CallVoidMethod(data, api.write_string, jpkg);
  env->CallVoidMethod(data, api.write_string, jproc);
  env->CallVoidMethod(data, api.write_int, app.user_id);
  env->CallVoidMethod(data, api.write_int, app.isolated ? 1 : 0);
  env->CallVoidMethod(data, api.write_int, app.child_zygote ? 1 : 0);
  env->CallVoidMethod(data, api.write_int, app.has_data_dir ? 1 : 0);
  if (env->ExceptionCheck()) {
    clear_exception(env, "Parcel.write(should_inject)");
    reason = "parcel_write_failed";
    goto cleanup;
  }

  if (!env->CallBooleanMethod(agent_binder, api.binder_transact, kTransShouldInject, data, reply, 0)) {
    clear_exception(env, "IBinder.transact(should_inject)");
    reason = "binder_transact_false";
    goto cleanup;
  }
  if (env->ExceptionCheck()) {
    clear_exception(env, "IBinder.transact(should_inject_exception)");
    reason = "binder_transact_exception";
    goto cleanup;
  }

  env->CallVoidMethod(reply, api.read_exception);
  if (env->ExceptionCheck()) {
    clear_exception(env, "Parcel.readException(should_inject)");
    reason = "binder_remote_exception";
    goto cleanup;
  }

  {
    jint allow_flag = env->CallIntMethod(reply, api.read_int);
    if (env->ExceptionCheck()) {
      clear_exception(env, "Parcel.readInt(should_inject)");
      reason = "binder_read_allow_failed";
      goto cleanup;
    }
    jstring jreason = static_cast<jstring>(env->CallObjectMethod(reply, api.read_string));
    if (env->ExceptionCheck()) {
      clear_exception(env, "Parcel.readString(should_inject)");
      reason = "binder_read_reason_failed";
      goto cleanup;
    }
    if (jreason != nullptr) {
      std::string remote_reason = trim_ascii(jstring_to_utf(env, jreason));
      if (!remote_reason.empty()) {
        reason = remote_reason;
      }
      env->DeleteLocalRef(jreason);
    }
    allow = (allow_flag != 0);
  }

cleanup:
  if (jdesc != nullptr) {
    env->DeleteLocalRef(jdesc);
  }
  if (jpkg != nullptr) {
    env->DeleteLocalRef(jpkg);
  }
  if (jproc != nullptr) {
    env->DeleteLocalRef(jproc);
  }
  env->CallVoidMethod(data, api.parcel_recycle);
  clear_exception(env, "Parcel.recycle(data)");
  env->CallVoidMethod(reply, api.parcel_recycle);
  clear_exception(env, "Parcel.recycle(reply)");
  env->DeleteLocalRef(data);
  env->DeleteLocalRef(reply);
  release_parcel_api(env, &api);

  if (reason_out != nullptr) {
    *reason_out = reason;
  }
  return allow;
}

bool transact_get_daemon_binder(JNIEnv* env, jobject agent_binder) {
  if (env == nullptr || agent_binder == nullptr) {
    return false;
  }
  ParcelApi api;
  if (!resolve_parcel_api(env, &api)) {
    return false;
  }

  jobject data = env->CallStaticObjectMethod(api.parcel_cls, api.parcel_obtain);
  clear_exception(env, "Parcel.obtain(data_binder)");
  jobject reply = env->CallStaticObjectMethod(api.parcel_cls, api.parcel_obtain);
  clear_exception(env, "Parcel.obtain(reply_binder)");
  if (data == nullptr || reply == nullptr) {
    if (data != nullptr) {
      env->DeleteLocalRef(data);
    }
    if (reply != nullptr) {
      env->DeleteLocalRef(reply);
    }
    release_parcel_api(env, &api);
    return false;
  }

  bool ok = false;
  jstring jdesc = env->NewStringUTF(kAgentDescriptor);
  if (jdesc == nullptr) {
    clear_exception(env, "NewStringUTF(get_daemon_binder)");
    goto cleanup;
  }

  env->CallVoidMethod(data, api.write_interface_token, jdesc);
  if (env->ExceptionCheck()) {
    clear_exception(env, "Parcel.write(get_daemon_binder)");
    goto cleanup;
  }
  if (!env->CallBooleanMethod(agent_binder, api.binder_transact, kTransGetDaemonBinder, data, reply, 0)) {
    clear_exception(env, "IBinder.transact(get_daemon_binder)");
    goto cleanup;
  }
  if (env->ExceptionCheck()) {
    clear_exception(env, "IBinder.transact(get_daemon_binder_exception)");
    goto cleanup;
  }
  env->CallVoidMethod(reply, api.read_exception);
  if (env->ExceptionCheck()) {
    clear_exception(env, "Parcel.readException(get_daemon_binder)");
    goto cleanup;
  }
  {
    jobject daemon_binder = env->CallObjectMethod(reply, api.read_strong_binder);
    if (env->ExceptionCheck()) {
      clear_exception(env, "Parcel.readStrongBinder(get_daemon_binder)");
      goto cleanup;
    }
    ok = (daemon_binder != nullptr);
    if (daemon_binder != nullptr) {
      env->DeleteLocalRef(daemon_binder);
    }
  }

cleanup:
  if (jdesc != nullptr) {
    env->DeleteLocalRef(jdesc);
  }
  env->CallVoidMethod(data, api.parcel_recycle);
  clear_exception(env, "Parcel.recycle(data_binder)");
  env->CallVoidMethod(reply, api.parcel_recycle);
  clear_exception(env, "Parcel.recycle(reply_binder)");
  env->DeleteLocalRef(data);
  env->DeleteLocalRef(reply);
  release_parcel_api(env, &api);
  return ok;
}

bool has_feature(JNIEnv* env, jobjectArray features, const char* expected) {
  if (env == nullptr || features == nullptr || expected == nullptr || expected[0] == '\0') {
    return false;
  }
  jsize n = env->GetArrayLength(features);
  for (jsize i = 0; i < n; ++i) {
    jobject obj = env->GetObjectArrayElement(features, i);
    if (obj == nullptr) {
      continue;
    }
    jstring item = static_cast<jstring>(obj);
    std::string value = trim_ascii(jstring_to_utf(env, item));
    env->DeleteLocalRef(item);
    if (value == expected) {
      return true;
    }
  }
  return false;
}

bool transact_get_info(JNIEnv* env, jobject agent_binder, std::string* reason_out) {
  if (reason_out != nullptr) {
    *reason_out = "agent_info_failed";
  }
  if (env == nullptr || agent_binder == nullptr) {
    return false;
  }
  ParcelApi api;
  if (!resolve_parcel_api(env, &api)) {
    if (reason_out != nullptr) {
      *reason_out = "parcel_api_missing";
    }
    return false;
  }
  jobject data = env->CallStaticObjectMethod(api.parcel_cls, api.parcel_obtain);
  clear_exception(env, "Parcel.obtain(info_data)");
  jobject reply = env->CallStaticObjectMethod(api.parcel_cls, api.parcel_obtain);
  clear_exception(env, "Parcel.obtain(info_reply)");
  if (data == nullptr || reply == nullptr) {
    if (reason_out != nullptr) {
      *reason_out = "parcel_alloc_failed";
    }
    if (data != nullptr) {
      env->DeleteLocalRef(data);
    }
    if (reply != nullptr) {
      env->DeleteLocalRef(reply);
    }
    release_parcel_api(env, &api);
    return false;
  }

  bool ok = false;
  std::string reason = "agent_info_failed";
  jstring jdesc = env->NewStringUTF(kAgentDescriptor);
  if (jdesc == nullptr) {
    clear_exception(env, "NewStringUTF(get_info)");
    reason = "jni_string_alloc_failed";
    goto cleanup;
  }

  env->CallVoidMethod(data, api.write_interface_token, jdesc);
  if (env->ExceptionCheck()) {
    clear_exception(env, "Parcel.write(get_info)");
    reason = "parcel_write_failed";
    goto cleanup;
  }
  if (!env->CallBooleanMethod(agent_binder, api.binder_transact, kTransGetInfo, data, reply, 0)) {
    clear_exception(env, "IBinder.transact(get_info)");
    reason = "binder_transact_false";
    goto cleanup;
  }
  if (env->ExceptionCheck()) {
    clear_exception(env, "IBinder.transact(get_info_exception)");
    reason = "binder_transact_exception";
    goto cleanup;
  }
  env->CallVoidMethod(reply, api.read_exception);
  if (env->ExceptionCheck()) {
    clear_exception(env, "Parcel.readException(get_info)");
    reason = "binder_remote_exception";
    goto cleanup;
  }

  {
    jint version = env->CallIntMethod(reply, api.read_int);
    if (env->ExceptionCheck()) {
      clear_exception(env, "Parcel.readInt(get_info)");
      reason = "binder_read_version_failed";
      goto cleanup;
    }
    jstring iface = static_cast<jstring>(env->CallObjectMethod(reply, api.read_string));
    if (env->ExceptionCheck()) {
      clear_exception(env, "Parcel.readString(get_info_iface)");
      reason = "binder_read_interface_failed";
      goto cleanup;
    }
    jobjectArray features = static_cast<jobjectArray>(env->CallObjectMethod(reply, api.create_string_array));
    if (env->ExceptionCheck()) {
      clear_exception(env, "Parcel.createStringArray(get_info)");
      if (iface != nullptr) {
        env->DeleteLocalRef(iface);
      }
      reason = "binder_read_features_failed";
      goto cleanup;
    }

    std::string iface_name = trim_ascii(jstring_to_utf(env, iface));
    bool has_daemon = has_feature(env, features, kAgentFeatureDaemonBinder);
    bool has_scope = has_feature(env, features, kAgentFeatureScopeDecider);
    if (iface != nullptr) {
      env->DeleteLocalRef(iface);
    }
    if (features != nullptr) {
      env->DeleteLocalRef(features);
    }
    if (version < 1) {
      reason = "agent_version_unsupported";
      goto cleanup;
    }
    if (iface_name.empty()) {
      reason = "agent_interface_missing";
      goto cleanup;
    }
    if (!has_daemon || !has_scope) {
      reason = "agent_feature_missing";
      goto cleanup;
    }
    ok = true;
    reason = "agent_info_ok";
  }

cleanup:
  if (jdesc != nullptr) {
    env->DeleteLocalRef(jdesc);
  }
  env->CallVoidMethod(data, api.parcel_recycle);
  clear_exception(env, "Parcel.recycle(info_data)");
  env->CallVoidMethod(reply, api.parcel_recycle);
  clear_exception(env, "Parcel.recycle(info_reply)");
  env->DeleteLocalRef(data);
  env->DeleteLocalRef(reply);
  release_parcel_api(env, &api);
  if (reason_out != nullptr) {
    *reason_out = reason;
  }
  return ok;
}

class DsapiZygiskModule : public zygisk::ModuleBase {
 public:
  void onLoad(zygisk::Api* api, JNIEnv* env) override {
    api_ = api;
    env_ = env;
    append_log("zygisk_loader_state=loaded api=5");
  }

  void preAppSpecialize(zygisk::AppSpecializeArgs* args) override {
    if (api_ == nullptr || env_ == nullptr || args == nullptr) {
      append_log("zygisk_loader_state=pre_app_invalid_context");
      return;
    }

    AppMeta app = read_app_meta(env_, args);
    std::string reason;
    bool allow = false;

    std::string service_name = read_env_service_name();
    jobject agent_binder = query_service_binder(env_, service_name);
    if (agent_binder == nullptr) {
      reason = "zygote_agent_missing";
      allow = false;
    } else {
      if (!transact_get_info(env_, agent_binder, &reason)) {
        allow = false;
      } else {
        allow = transact_should_inject(env_, agent_binder, app, &reason);
        if (allow && !transact_get_daemon_binder(env_, agent_binder)) {
          allow = false;
          reason = "daemon_binder_missing";
        }
      }
      env_->DeleteLocalRef(agent_binder);
    }

    api_->setOption(zygisk::DLCLOSE_MODULE_LIBRARY);
    if (!allow) {
      api_->setOption(zygisk::FORCE_DENYLIST_UNMOUNT);
    }

    std::string line = "zygisk_loader_decision allow=";
    line += allow ? "1" : "0";
    line += " package=" + sanitize_token(app.package_name);
    line += " process=" + sanitize_token(app.process_name.empty() ? "-" : app.process_name);
    line += " user=" + std::to_string(app.user_id);
    line += " isolated=" + std::string(app.isolated ? "1" : "0");
    line += " child_zygote=" + std::string(app.child_zygote ? "1" : "0");
    line += " has_data_dir=" + std::string(app.has_data_dir ? "1" : "0");
    line += " reason=" + sanitize_token(reason.empty() ? "-" : reason);
    line += " agent_service=" + sanitize_token(service_name);
    append_log(line);
  }

 private:
  zygisk::Api* api_ = nullptr;
  JNIEnv* env_ = nullptr;
};

}  // namespace

REGISTER_ZYGISK_MODULE(DsapiZygiskModule)

extern "C" __attribute__((visibility("default"))) int dsapi_should_inject(
    const char* package_name,
    int user_id,
    int isolated,
    int child_zygote,
    int has_data_dir) {
  (void) package_name;
  (void) user_id;
  if (isolated != 0 || child_zygote != 0 || has_data_dir == 0) {
    return 0;
  }
  return 1;
}
