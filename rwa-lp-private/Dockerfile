# Build context is a staging directory made by deploy.sh: public/ = rwa-lp/service, private/ = rwa-lp-private/private
FROM python:3.12-slim
WORKDIR /srv
COPY public/requirements.txt .
COPY private/requirements.txt private-requirements.txt
RUN pip install --no-cache-dir -r requirements.txt -r private-requirements.txt
COPY public/app ./app
COPY private ./private
CMD exec uvicorn private.main:app --host 0.0.0.0 --port ${PORT:-8080} --workers 1
