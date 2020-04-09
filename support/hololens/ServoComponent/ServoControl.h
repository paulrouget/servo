#pragma once

#include "ServoControl.g.h"

namespace winrt::ServoComponent::implementation
{
    struct ServoControl : ServoControlT<ServoControl>
    {
        ServoControl() = default;

        int32_t MyProperty();
        void MyProperty(int32_t value);
    };
}

namespace winrt::ServoComponent::factory_implementation
{
    struct ServoControl : ServoControlT<ServoControl, implementation::ServoControl>
    {
    };
}
