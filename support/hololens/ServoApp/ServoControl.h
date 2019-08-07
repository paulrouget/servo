#pragma once
#include "ServoControl.g.h"
#include "OpenGLES.h"
#include "Servo.h"

namespace winrt::ServoApp::implementation {
struct ServoControl : ServoControlT<ServoControl>, public servo::ServoDelegate {

  ServoControl() {
    DefaultStyleKey(winrt::box_value(L"ServoApp.ServoControl"));
  }

  void OnPointerPressed(Windows::UI::Xaml::Input::PointerRoutedEventArgs const &) const;
  void OnApplyTemplate();

  static void
  OnLabelChanged(Windows::UI::Xaml::DependencyObject const &,
                 Windows::UI::Xaml::DependencyPropertyChangedEventArgs const &);

    void Shutdown();

  virtual void WakeUp();
  virtual void OnLoadStarted();
  virtual void OnLoadEnded();
  virtual void OnHistoryChanged(bool, bool);
  virtual void OnShutdownComplete();
  virtual void OnTitleChanged(std::wstring);
  virtual void OnAlert(std::wstring);
  virtual void OnURLChanged(std::wstring);
  virtual void Flush();
  virtual void MakeCurrent();
  virtual bool OnAllowNavigation(std::wstring);
  virtual void OnAnimatingChanged(bool);

private:
  Windows::UI::Xaml::Controls::SwapChainPanel ServoControl::Panel();
  void CreateRenderSurface();
  void DestroyRenderSurface();
  void RecoverFromLostDevice();

  void StartRenderLoop();
  void StopRenderLoop();
  void Loop();
  void OnVisibilityChanged(
      Windows::UI::Core::CoreWindow const &,
      Windows::UI::Core::VisibilityChangedEventArgs const &args);

  void
  OnSurfaceClicked(Windows::Foundation::IInspectable const &,
                   Windows::UI::Xaml::Input::PointerRoutedEventArgs const &);

  void OnSurfaceManipulationDelta(
      IInspectable const &,
      Windows::UI::Xaml::Input::ManipulationDeltaRoutedEventArgs const &e);

  template <typename Callable> void RunOnUIThread(Callable);
  void RunOnGLThread(std::function<void()>);

  static Windows::UI::Xaml::DependencyProperty m_labelProperty;

  std::unique_ptr<servo::Servo> mServo;
  EGLSurface mRenderSurface{EGL_NO_SURFACE};
  OpenGLES mOpenGLES;
  bool mAnimating = false;
  bool mLooping = false; // std::unique_ptr<servo::Servo> mServo;
  std::vector<std::function<void()>> mTasks;
  CRITICAL_SECTION mGLLock;
  CONDITION_VARIABLE mGLCondVar;
  std::unique_ptr<Concurrency::task<void>> mLoopTask;
};
} // namespace winrt::ServoApp::implementation

namespace winrt::ServoApp::factory_implementation {
struct ServoControl
    : ServoControlT<ServoControl, implementation::ServoControl> {};
} // namespace winrt::ServoApp::factory_implementation
