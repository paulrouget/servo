#include "pch.h"
#include "ServoControl.h"
#include "ServoControl.g.cpp"

namespace winrt::ServoApp::implementation
{
  Windows::UI::Xaml::DependencyProperty ServoControl::m_labelProperty =
    Windows::UI::Xaml::DependencyProperty::Register(
      L"Label",
      winrt::xaml_typename<winrt::hstring>(),
      winrt::xaml_typename<ServoApp::ServoControl>(),
      Windows::UI::Xaml::PropertyMetadata{
        winrt::box_value(L"default label"),
        Windows::UI::Xaml::PropertyChangedCallback{
        &ServoControl::OnLabelChanged
      } }
  );

  void ServoControl::OnLabelChanged(Windows::UI::Xaml::DependencyObject const& d, Windows::UI::Xaml::DependencyPropertyChangedEventArgs const& /* e */)
  {
    if (ServoApp::ServoControl theControl{ d.try_as<ServoApp::ServoControl>() })
    {
      // Call members of the projected type via theControl.

      ServoApp::implementation::ServoControl* ptr{ winrt::get_self<ServoApp::implementation::ServoControl>(theControl) };
      // Call members of the implementation type via ptr.
    }
  }
}
