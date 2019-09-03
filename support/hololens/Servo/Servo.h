#pragma once

#include "Servo.g.h"

namespace winrt::Servo::implementation
{
    struct Servo : ServoT<Servo>
    {
        Servo() = default;

        int32_t MyProperty();
        void MyProperty(int32_t value);
    };
}

namespace winrt::Servo::factory_implementation
{
    struct Servo : ServoT<Servo, implementation::Servo>
    {
    };
}
