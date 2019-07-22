#include "pch.h"
#include "Servo.h"

void on_load_started() {}
void on_load_ended() {}
void on_history_changed(bool, bool) {}
void on_shutdown_complete() {}

std::function<void()> Servo::sFlush = []() {};
std::function<void()> Servo::sMakeCurrent = []() {};
std::function<void()> Servo::sFlushXR = []() {};
std::function<void()> Servo::sMakeCurrentXR = []() {};
std::function<void()> Servo::sWakeUp = []() {};
std::function<void()> Servo::sToImmersiveMode = []() {};

std::function<void(std::wstring const &)> Servo::sOnAlert =
    [](std::wstring const &) {};
std::function<void(std::wstring const &)> Servo::sOnTitleChanged =
    [](std::wstring const &) {};
std::function<void(std::wstring const &)> Servo::sOnURLChanged =
    [](std::wstring const &) {};

bool Servo::sAnimating = false;

std::wstring char2w(const char *c_str) {
  auto str = std::string(c_str);
  int size_needed =
      MultiByteToWideChar(CP_UTF8, 0, &str[0], (int)str.size(), NULL, 0);
  std::wstring str2(size_needed, 0);
  MultiByteToWideChar(CP_UTF8, 0, &str[0], (int)str.size(), &str2[0],
                      size_needed);
  return str2;
}

void on_alert(const char *message) { Servo::sOnAlert(char2w(message)); }

void on_title_changed(const char *title) {
  Servo::sOnTitleChanged(char2w(title));
}

void on_url_changed(const char *url) { Servo::sOnURLChanged(char2w(url)); }

void flush() { Servo::sFlush(); }

void make_current() { Servo::sMakeCurrent(); }

void flush_xr() { Servo::sFlushXR(); }

void make_current_xr() { Servo::sMakeCurrentXR(); }

void wakeup() { Servo::sWakeUp(); }

bool on_allow_navigation(const char *url) { return true; };

void to_immersive_mode() { Servo::sToImmersiveMode(); }

void on_animating_changed(bool aAnimating) { Servo::sAnimating = aAnimating; }

Servo::Servo(GLsizei width, GLsizei height)
    : mWindowHeight(height), mWindowWidth(width) {

  CInitOptions o;
  o.args = NULL;
  o.url = "http://paulrouget.com/webgl-to-webvr/webxr.html";
  o.width = mWindowWidth;
  o.height = mWindowHeight;
  o.density = 1.0;
  o.enable_subpixel_text_antialiasing = false;
  o.vr_pointer = NULL;

  CHostCallbacks c;
  c.flush = &flush;
  c.make_current = &make_current;
  c.flush_xr = &flush_xr;
  c.make_current_xr = &make_current_xr;
  c.on_alert = &on_alert;
  c.on_load_started = &on_load_started;
  c.on_load_ended = &on_load_ended;
  c.on_title_changed = &on_title_changed;
  c.on_url_changed = &on_url_changed;
  c.on_history_changed = &on_history_changed;
  c.on_animating_changed = &on_animating_changed;
  c.on_shutdown_complete = &on_shutdown_complete;
  c.on_allow_navigation = &on_allow_navigation;
  c.to_immersive_mode = &to_immersive_mode;

  init_with_egl(o, &wakeup, c);
}

Servo::~Servo() { deinit(); }

void Servo::PerformUpdates() { perform_updates(); }

void Servo::SetBatchMode(bool mode) { set_batch_mode(mode); }

void Servo::GoForward() { go_forward(); }

void Servo::GoBack() { go_back(); }

void Servo::SetSize(GLsizei width, GLsizei height) {
  if (width != mWindowWidth || height != mWindowHeight) {
    mWindowWidth = width;
    mWindowHeight = height;
    resize(mWindowWidth, mWindowHeight);
  }
}

void Servo::Click(float x, float y) { click(x, y); }
