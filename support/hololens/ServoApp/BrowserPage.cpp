/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#include "pch.h"
#include "logs.h"
#include "BrowserPage.h"
#include "BrowserPage.g.cpp"
#include "ImmersiveView.h"
#include "OpenGLES.h"

using namespace std::placeholders;
using namespace winrt::Windows::UI::Xaml;
using namespace winrt::Windows::UI::Core;
using namespace winrt::Windows::UI::ViewManagement;
using namespace winrt::Windows::Foundation;
using namespace winrt::Windows::Graphics::Holographic;
using namespace concurrency;
using namespace servo;

namespace winrt::ServoApp::implementation {
BrowserPage::BrowserPage() {
  log("BrowserPage::BrowserPage()");
  InitializeComponent();
}

void BrowserPage::Shutdown() {
  log("BrowserPage::Shutdown()");
  // FIXME: control.Shutdown();
}


/**** USER INTERACTIONS WITH UI ****/

void BrowserPage::OnBackButtonClicked(IInspectable const &,
                                      RoutedEventArgs const &) {
  //RunOnGLThread([=] { mServo->GoBack(); });
}

void BrowserPage::OnForwardButtonClicked(IInspectable const &,
                                         RoutedEventArgs const &) {
  //RunOnGLThread([=] { mServo->GoForward(); });
}

void BrowserPage::OnReloadButtonClicked(IInspectable const &,
                                        RoutedEventArgs const &) {
  //RunOnGLThread([=] { mServo->Reload(); });
}

void BrowserPage::OnStopButtonClicked(IInspectable const &,
                                      RoutedEventArgs const &) {
  //RunOnGLThread([=] { mServo->Stop(); });
}

void BrowserPage::OnURLEdited(IInspectable const & sender,
  Input::KeyRoutedEventArgs const & e) {
  if (e.Key() == Windows::System::VirtualKey::Enter) {
    // SwapChainPanel can't be focus. Focusing the stopButton for now.
    // We'll need to build a custom element to make the swapchain focusable.
  }
}


void BrowserPage::OnImmersiveButtonClicked(IInspectable const &,
                                           RoutedEventArgs const &) {
  if (HolographicSpace::IsAvailable()) {
    log("Holographic space available");
    auto v =
        winrt::Windows::ApplicationModel::Core::CoreApplication::CreateNewView(
            mImmersiveViewSource);
    auto parentId = ApplicationView::GetForCurrentView().Id();
    v.Dispatcher().RunAsync(CoreDispatcherPriority::Normal, [=] {
      auto winId = ApplicationView::GetForCurrentView().Id();
      ApplicationViewSwitcher::SwitchAsync(winId, parentId);
      log("Immersive view started");
    });
  } else {
    log("Holographic space not available");
  }
}

} // namespace winrt::ServoApp::implementation
