#pragma once
#include "ServoControl.g.h"

namespace winrt::ServoApp::implementation
{
  struct ServoControl : ServoControlT<ServoControl>
  {
    ServoControl() { DefaultStyleKey(winrt::box_value(L"ServoApp.ServoControl")); }

    winrt::hstring Label()
    {
      return winrt::unbox_value<winrt::hstring>(GetValue(m_labelProperty));
    }

    void Label(winrt::hstring const& value)
    {
      SetValue(m_labelProperty, winrt::box_value(value));
    }

    static Windows::UI::Xaml::DependencyProperty LabelProperty() { return m_labelProperty; }

    static void OnLabelChanged(Windows::UI::Xaml::DependencyObject const&, Windows::UI::Xaml::DependencyPropertyChangedEventArgs const&);

  private:
    static Windows::UI::Xaml::DependencyProperty m_labelProperty;
  };
}
namespace winrt::ServoApp::factory_implementation
{
  struct ServoControl : ServoControlT<ServoControl, implementation::ServoControl>
  {
  };
}
