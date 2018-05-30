package org.mozilla.servo;

import android.content.Context;
import android.opengl.GLSurfaceView;
import android.util.AttributeSet;

public class ServoGLView extends GLSurfaceView {

    private final ServoGLRenderer mRenderer;

    public ServoGLView(Context context, AttributeSet attrs) {
        super(context, attrs);
        setEGLContextClientVersion(3);
        setEGLConfigChooser(8, 8, 8, 8, 24, 0);
        mRenderer = new ServoGLRenderer();
        setRenderer(mRenderer);
        //setRenderMode(GLSurfaceView.RENDERMODE_WHEN_DIRTY);
    }

}